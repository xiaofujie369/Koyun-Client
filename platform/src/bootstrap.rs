use crate::{
    api::{ApiError, ApiState},
    crypto::decode_key,
    scope::TenantId,
    store::{Store, StoreError},
};
use axum::{
    Json,
    extract::{Path, State},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

pub struct BootstrapConfig {
    signing_key: SigningKey,
    key_id: String,
    origin: String,
    pub tenant_id: TenantId,
    pub brand_id: Uuid,
}
#[derive(Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub key_id: String,
    pub payload: String,
    pub signature: String,
}
impl BootstrapConfig {
    pub fn new(
        seed_hex: &str,
        key_id: &str,
        origin: &str,
        tenant_id: TenantId,
        brand_id: Uuid,
    ) -> Result<Self, StoreError> {
        let seed = decode_key(seed_hex).map_err(|_| StoreError::InvalidInput)?;
        let url = reqwest::Url::parse(origin).map_err(|_| StoreError::InvalidInput)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || key_id.is_empty()
            || key_id.len() > 64
            || !key_id
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || b"_-".contains(&v))
            || brand_id.is_nil()
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(Self {
            signing_key: SigningKey::from_bytes(&seed),
            key_id: key_id.into(),
            origin: url.as_str().trim_end_matches('/').into(),
            tenant_id,
            brand_id,
        })
    }
    pub fn public_key(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.signing_key.verifying_key().to_bytes())
    }
    pub fn sign(&self, payload: &Value) -> Result<SignedEnvelope, StoreError> {
        let bytes = serde_json::to_vec(payload).map_err(|_| StoreError::Unavailable)?;
        Ok(SignedEnvelope {
            key_id: self.key_id.clone(),
            payload: URL_SAFE_NO_PAD.encode(&bytes),
            signature: URL_SAFE_NO_PAD.encode(self.signing_key.sign(&bytes).to_bytes()),
        })
    }
    pub async fn document(&self, state: &ApiState) -> Result<SignedEnvelope, StoreError> {
        let (brand, version) = state
            .store
            .bootstrap_brand(&self.tenant_id, self.brand_id)
            .await?;
        let now = Utc::now().timestamp();
        let allow_other_tenants = brand["allow_other_tenants"].as_bool().unwrap_or(false);
        let mut tenants = state.store.public_tenants().await?;
        tenants.retain(|entry| {
            if !allow_other_tenants && entry["id"] != self.tenant_id.as_str() {
                return false;
            }
            entry["id"]
                .as_str()
                .and_then(|id| TenantId::parse(id).ok())
                .is_some_and(|id| state.adapters.get(&id).is_ok())
        });
        self.sign(&json!({"format":"koyun-bootstrap-v1","key_id":self.key_id,"version":version,"issued_at":now,"expires_at":now+86400,"tenant_id":self.tenant_id.as_str(),"brand_id":self.brand_id,"brand":brand,"tenants":tenants,"endpoints":[self.origin],"ws_endpoint":format!("wss://{}/v1/realtime/ws",self.origin.trim_start_matches("https://")),"features":{"local_mode":true,"managed_profile":true,"device_management":true,"multi_tenant":true,"realtime":true},"poll_interval_seconds":30}))
    }
}
impl Store {
    pub async fn bootstrap_brand(
        &self,
        tenant: &TenantId,
        id: Uuid,
    ) -> Result<(Value, i64), StoreError> {
        let mut tx = self.begin(tenant).await?;
        let row=sqlx::query("SELECT b.name,b.app_name,b.logo_url,b.icon_url,b.theme_config,b.website_url,b.support_url,b.privacy_url,b.terms_url,b.allow_other_tenants,t.policy_version FROM brands b JOIN tenants t ON t.id=b.tenant_id WHERE b.tenant_id=$1 AND b.id=$2 AND b.status='active' AND t.status='active'")
            .bind(tenant.as_str()).bind(id).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        let brand = json!({"name":row.get::<String,_>("name"),"app_name":row.get::<String,_>("app_name"),"logo_url":row.get::<Option<String>,_>("logo_url"),"icon_url":row.get::<Option<String>,_>("icon_url"),"theme":row.get::<Value,_>("theme_config"),"website_url":row.get::<Option<String>,_>("website_url"),"support_url":row.get::<Option<String>,_>("support_url"),"privacy_url":row.get::<Option<String>,_>("privacy_url"),"terms_url":row.get::<Option<String>,_>("terms_url"),"allow_other_tenants":row.get::<bool,_>("allow_other_tenants"),"allow_local_mode":true});
        let version = row.get("policy_version");
        tx.commit().await?;
        Ok((brand, version))
    }
}
pub(crate) async fn bootstrap(State(state): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let config = state.bootstrap.as_ref().ok_or(StoreError::Unavailable)?;
    Ok(Json(json!({"data":config.document(&state).await?})))
}
pub(crate) async fn brand(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let config = state.bootstrap.as_ref().ok_or(StoreError::Unavailable)?;
    if id != config.brand_id {
        return Err(StoreError::NotFound.into());
    }
    Ok(Json(json!({"data":config.document(&state).await?})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signature_covers_exact_payload_and_tampering_is_rejected() {
        let config = BootstrapConfig::new(
            &"22".repeat(32),
            "root-1",
            "https://example.com",
            TenantId::parse("demo").unwrap(),
            Uuid::new_v4(),
        )
        .unwrap();
        let envelope = config
            .sign(&json!({"version":1,"local_mode":true}))
            .unwrap();
        let signature = ed25519_dalek::Signature::from_slice(
            &URL_SAFE_NO_PAD.decode(envelope.signature).unwrap(),
        )
        .unwrap();
        let mut payload = URL_SAFE_NO_PAD.decode(envelope.payload).unwrap();
        assert!(
            config
                .signing_key
                .verifying_key()
                .verify_strict(&payload, &signature)
                .is_ok()
        );
        payload[0] ^= 1;
        assert!(
            config
                .signing_key
                .verifying_key()
                .verify_strict(&payload, &signature)
                .is_err()
        );
        assert_eq!(
            URL_SAFE_NO_PAD.decode(config.public_key()).unwrap().len(),
            32
        );
    }
    #[test]
    fn bootstrap_requires_clean_https_origin() {
        for url in [
            "http://example.com",
            "https://a:b@example.com",
            "https://example.com/path",
            "https://example.com?token=x",
        ] {
            assert!(
                BootstrapConfig::new(
                    &"22".repeat(32),
                    "root-1",
                    url,
                    TenantId::parse("demo").unwrap(),
                    Uuid::new_v4()
                )
                .is_err()
            );
        }
    }
}
