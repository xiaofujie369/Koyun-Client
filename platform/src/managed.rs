use crate::{
    panel::{Entitlement, ProfileDocument},
    store::{Principal, Store, StoreError},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

#[derive(Serialize)]
pub struct ProfileState {
    pub id: Uuid,
    pub tenant_id: String,
    pub account_id: Uuid,
    pub version: i64,
    pub etag: Option<String>,
    pub status: String,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
}
impl ProfileState {
    fn from_row(row: &PgRow) -> Self {
        Self {
            id: row.get("id"),
            tenant_id: row.get("tenant_id"),
            account_id: row.get("account_id"),
            version: row.get("version"),
            etag: row.get("etag"),
            status: row.get("status"),
            last_sync_at: row.get("last_sync_at"),
            last_success_at: row.get("last_success_at"),
        }
    }
}

impl Store {
    pub async fn managed_state(&self, actor: &Principal) -> Result<ProfileState, StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let row=sqlx::query("INSERT INTO managed_profiles(id,tenant_id,account_id) VALUES($1,$2,$3) ON CONFLICT(tenant_id,account_id) DO UPDATE SET account_id=EXCLUDED.account_id RETURNING *")
            .bind(Uuid::new_v4()).bind(actor.tenant_id.as_str()).bind(actor.account_id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(ProfileState::from_row(&row))
    }

    pub async fn cache_profile(
        &self,
        actor: &Principal,
        document: &ProfileDocument,
        started: DateTime<Utc>,
    ) -> Result<ProfileState, StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let row = sqlx::query(
            "SELECT * FROM managed_profiles WHERE tenant_id=$1 AND account_id=$2 FOR UPDATE",
        )
        .bind(actor.tenant_id.as_str())
        .bind(actor.account_id)
        .fetch_one(&mut *tx)
        .await?;
        let previous = ProfileState::from_row(&row);
        if previous.last_sync_at.is_some_and(|last| last > started) {
            return Ok(previous);
        }
        let changed =
            previous.etag.as_deref() != Some(document.etag()) || previous.status != "active";
        let version = previous.version + if changed { 1 } else { 0 };
        if changed {
            let encrypted = self
                .key
                .encrypt(
                    document.content(),
                    format!(
                        "profile:{}:{}:{}",
                        actor.tenant_id.as_str(),
                        previous.id,
                        version
                    )
                    .as_bytes(),
                )
                .map_err(|_| StoreError::Unavailable)?;
            sqlx::query("INSERT INTO profile_cache(tenant_id,profile_id,version,content_encrypted,etag) VALUES($1,$2,$3,$4,$5)")
                .bind(actor.tenant_id.as_str()).bind(previous.id).bind(version).bind(encrypted).bind(document.etag()).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO realtime_events(id,tenant_id,account_id,type,version,payload) VALUES($1,$2,$3,'profile.changed',$4,$5)")
                .bind(Uuid::new_v4()).bind(actor.tenant_id.as_str()).bind(actor.account_id).bind(version).bind(serde_json::json!({"profile_id":previous.id})).execute(&mut *tx).await?;
            sqlx::query(
                "DELETE FROM profile_cache WHERE tenant_id=$1 AND profile_id=$2 AND version<$3",
            )
            .bind(actor.tenant_id.as_str())
            .bind(previous.id)
            .bind(version - 1)
            .execute(&mut *tx)
            .await?;
        }
        let row=sqlx::query("UPDATE managed_profiles SET version=$3,etag=$4,status='active',last_sync_at=$5,last_success_at=now() WHERE tenant_id=$1 AND id=$2 RETURNING *")
            .bind(actor.tenant_id.as_str()).bind(previous.id).bind(version).bind(document.etag()).bind(started).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(ProfileState::from_row(&row))
    }

    pub async fn suspend_profile(&self, actor: &Principal) -> Result<(), StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let row=sqlx::query("UPDATE managed_profiles SET status='suspended',version=version+1,last_sync_at=now() WHERE tenant_id=$1 AND account_id=$2 AND status<>'suspended' RETURNING id,version")
            .bind(actor.tenant_id.as_str()).bind(actor.account_id).fetch_optional(&mut *tx).await?;
        if let Some(row) = row {
            sqlx::query("INSERT INTO realtime_events(id,tenant_id,account_id,type,version,payload) VALUES($1,$2,$3,'profile.changed',$4,$5)")
                .bind(Uuid::new_v4()).bind(actor.tenant_id.as_str()).bind(actor.account_id).bind(row.get::<i64,_>("version")).bind(serde_json::json!({"profile_id":row.get::<Uuid,_>("id"),"status":"suspended"})).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn managed_content(
        &self,
        actor: &Principal,
        id: Uuid,
    ) -> Result<(ProfileState, ProfileDocument), StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let row=sqlx::query("SELECT p.*,c.content_encrypted FROM managed_profiles p LEFT JOIN profile_cache c ON c.tenant_id=p.tenant_id AND c.profile_id=p.id AND c.version=p.version WHERE p.tenant_id=$1 AND p.account_id=$2 AND p.id=$3")
            .bind(actor.tenant_id.as_str()).bind(actor.account_id).bind(id).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        let state = ProfileState::from_row(&row);
        if state.status != "active" {
            return Err(StoreError::Forbidden);
        }
        let encrypted: Vec<u8> = row
            .try_get("content_encrypted")
            .map_err(|_| StoreError::Unavailable)?;
        let content = self
            .key
            .decrypt(
                &encrypted,
                format!(
                    "profile:{}:{}:{}",
                    actor.tenant_id.as_str(),
                    id,
                    state.version
                )
                .as_bytes(),
            )
            .map_err(|_| StoreError::Unavailable)?;
        let document =
            ProfileDocument::parse(content.to_vec()).map_err(|_| StoreError::Unavailable)?;
        if state.etag.as_deref() != Some(document.etag()) {
            return Err(StoreError::Unavailable);
        }
        tx.commit().await?;
        Ok((state, document))
    }

    pub async fn save_entitlement(
        &self,
        actor: &Principal,
        e: &Entitlement,
    ) -> Result<i64, StoreError> {
        let quota = i64::try_from(e.quota_bytes).map_err(|_| StoreError::InvalidInput)?;
        let upload = i64::try_from(e.upload_bytes).map_err(|_| StoreError::InvalidInput)?;
        let download = i64::try_from(e.download_bytes).map_err(|_| StoreError::InvalidInput)?;
        let limit = e
            .device_limit
            .map(i32::try_from)
            .transpose()
            .map_err(|_| StoreError::InvalidInput)?;
        let expires = e
            .expires_at
            .map(|v| {
                i64::try_from(v)
                    .ok()
                    .and_then(|v| DateTime::from_timestamp(v, 0))
                    .ok_or(StoreError::InvalidInput)
            })
            .transpose()?;
        let mut tx = self.begin(&actor.tenant_id).await?;
        let version:i64=sqlx::query_scalar("INSERT INTO entitlements(tenant_id,account_id,quota_bytes,upload_bytes,download_bytes,device_limit,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,account_id) DO UPDATE SET quota_bytes=EXCLUDED.quota_bytes,upload_bytes=EXCLUDED.upload_bytes,download_bytes=EXCLUDED.download_bytes,device_limit=EXCLUDED.device_limit,expires_at=EXCLUDED.expires_at,last_verified_at=now(),version=entitlements.version+CASE WHEN (entitlements.quota_bytes,entitlements.upload_bytes,entitlements.download_bytes,entitlements.device_limit,entitlements.expires_at) IS DISTINCT FROM (EXCLUDED.quota_bytes,EXCLUDED.upload_bytes,EXCLUDED.download_bytes,EXCLUDED.device_limit,EXCLUDED.expires_at) THEN 1 ELSE 0 END RETURNING version")
            .bind(actor.tenant_id.as_str()).bind(actor.account_id).bind(quota).bind(upload).bind(download).bind(limit).bind(expires).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(version)
    }

    pub async fn sync_versions(&self, actor: &Principal) -> Result<serde_json::Value, StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let row=sqlx::query("SELECT t.policy_version,t.offline_grace_seconds,COALESCE(p.version,0) AS profile_version,COALESCE(e.version,0) AS entitlement_version,COALESCE(p.status,'pending') AS profile_status FROM tenants t LEFT JOIN managed_profiles p ON p.tenant_id=t.id AND p.account_id=$2 LEFT JOIN entitlements e ON e.tenant_id=t.id AND e.account_id=$2 WHERE t.id=$1")
            .bind(actor.tenant_id.as_str()).bind(actor.account_id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(
            serde_json::json!({"tenant_id":actor.tenant_id.as_str(),"account_id":actor.account_id,"profile_version":row.get::<i64,_>("profile_version"),"entitlement_version":row.get::<i64,_>("entitlement_version"),"policy_version":row.get::<i64,_>("policy_version"),"offline_grace_seconds":row.get::<i32,_>("offline_grace_seconds"),"profile_status":row.get::<String,_>("profile_status"),"server_time":Utc::now()}),
        )
    }
}
