use crate::{
    crypto::{MasterKey, TokenKind, generate_token, inspect_token},
    panel::{PanelSession, PanelUser, Secret},
    scope::TenantId,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

pub struct Store {
    pool: PgPool,
    key: MasterKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Unavailable,
    Unauthorized,
    Forbidden,
    DeviceLimit,
    InvalidInput,
    NotFound,
}

impl From<sqlx::Error> for StoreError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Deserialize, Clone)]
pub struct DeviceInput {
    pub installation_id: Uuid,
    pub name: String,
    pub platform: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub account_id: Uuid,
    pub device_id: Uuid,
    pub tenant_id: String,
}

pub struct Principal {
    pub tenant_id: TenantId,
    pub account_id: Uuid,
    pub device_id: Uuid,
    pub session_id: Uuid,
}

#[derive(Serialize)]
pub struct DeviceView {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub struct TenantConnection {
    pub panel_type: String,
    pub base_url: Option<String>,
}

impl Store {
    pub async fn new(pool: PgPool, key: MasterKey) -> Result<Self, StoreError> {
        let unsafe_role: bool = sqlx::query_scalar(
            "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_one(&pool)
        .await?;
        if unsafe_role {
            return Err(StoreError::Forbidden);
        }
        Ok(Self { pool, key })
    }

    async fn begin(&self, tenant: &TenantId) -> Result<Transaction<'_, Postgres>, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
            .bind(tenant.as_str())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    pub async fn tenant_connection(
        &self,
        tenant: &TenantId,
    ) -> Result<TenantConnection, StoreError> {
        let row = sqlx::query(
            "SELECT panel_type,panel_base_url FROM tenants WHERE id=$1 AND status='active'",
        )
        .bind(tenant.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        Ok(TenantConnection {
            panel_type: row.get("panel_type"),
            base_url: row.get("panel_base_url"),
        })
    }

    pub async fn public_tenants(&self) -> Result<Vec<serde_json::Value>, StoreError> {
        Ok(sqlx::query("SELECT id,name FROM tenants WHERE status='active' ORDER BY name").fetch_all(&self.pool).await?
            .iter().map(|r| serde_json::json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name"),"login_enabled":true})).collect())
    }

    async fn license(
        tx: &mut Transaction<'_, Postgres>,
        tenant: &TenantId,
        platform: &str,
        now: DateTime<Utc>,
    ) -> Result<(i32, Option<i64>, Option<i64>), StoreError> {
        let row = sqlx::query("SELECT t.default_device_limit,l.max_users,l.max_active_devices FROM tenants t JOIN tenant_licenses l ON l.tenant_id=t.id WHERE t.id=$1 AND t.status='active' AND l.starts_at <= $2 AND ((l.status IN ('trial','active') AND (l.expires_at IS NULL OR l.expires_at>$2)) OR (l.status='grace' AND l.grace_until>$2)) AND CASE $3 WHEN 'linux' THEN l.allow_linux WHEN 'windows' THEN l.allow_windows WHEN 'android' THEN l.allow_android ELSE false END FOR UPDATE OF t")
            .bind(tenant.as_str()).bind(now).bind(platform).fetch_optional(&mut **tx).await?.ok_or(StoreError::Forbidden)?;
        Ok((
            row.get("default_device_limit"),
            row.get("max_users"),
            row.get("max_active_devices"),
        ))
    }

    pub async fn check_login_license(
        &self,
        tenant: &TenantId,
        platform: &str,
    ) -> Result<(), StoreError> {
        let mut tx = self.begin(tenant).await?;
        Self::license(&mut tx, tenant, platform, Utc::now()).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn login(
        &self,
        panel: &PanelSession,
        user: &PanelUser,
        device: &DeviceInput,
        entitlement_device_limit: Option<u32>,
    ) -> Result<TokenPair, StoreError> {
        if device.name.trim().is_empty()
            || device.name.len() > 128
            || device.capabilities.len() > 32
            || device.capabilities.iter().any(|v| v.len() > 64)
            || device.installation_id.is_nil()
        {
            return Err(StoreError::InvalidInput);
        }
        if user.disabled || panel.external_user_id != user.external_user_id {
            return Err(StoreError::Forbidden);
        }
        let now = Utc::now();
        let mut tx = self.begin(&panel.tenant_id).await?;
        let (limit, max_users, max_devices) =
            Self::license(&mut tx, &panel.tenant_id, &device.platform, now).await?;
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM managed_accounts WHERE tenant_id=$1 AND external_user_id=$2",
        )
        .bind(panel.tenant_id.as_str())
        .bind(&user.external_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.is_none() {
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM managed_accounts WHERE tenant_id=$1")
                    .bind(panel.tenant_id.as_str())
                    .fetch_one(&mut *tx)
                    .await?;
            if max_users.is_some_and(|max| count >= max) {
                return Err(StoreError::Forbidden);
            }
        }
        let account_id = existing.unwrap_or_else(Uuid::new_v4);
        let encrypted = self
            .key
            .encrypt(
                panel.credential.expose().as_bytes(),
                format!(
                    "panel_credential:{}:{}",
                    panel.tenant_id.as_str(),
                    account_id
                )
                .as_bytes(),
            )
            .map_err(|_| StoreError::Unavailable)?;
        let status:String=sqlx::query_scalar("INSERT INTO managed_accounts(id,tenant_id,external_user_id,email_normalized,panel_credential_encrypted) VALUES($1,$2,$3,$4,$5) ON CONFLICT(tenant_id,external_user_id) DO UPDATE SET email_normalized=EXCLUDED.email_normalized,panel_credential_encrypted=EXCLUDED.panel_credential_encrypted,last_login_at=now() RETURNING status")
            .bind(account_id).bind(panel.tenant_id.as_str()).bind(&user.external_user_id).bind(user.email.trim().to_lowercase()).bind(encrypted).fetch_one(&mut *tx).await?;
        if status != "active" {
            return Err(StoreError::Forbidden);
        }
        let installation_hash = Sha256::digest(device.installation_id.as_bytes()).to_vec();
        let found=sqlx::query("SELECT id,revoked_at FROM devices WHERE tenant_id=$1 AND account_id=$2 AND installation_hash=$3")
            .bind(panel.tenant_id.as_str()).bind(account_id).bind(&installation_hash).fetch_optional(&mut *tx).await?;
        let device_id = if let Some(row) = found {
            if row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_some() {
                return Err(StoreError::Forbidden);
            }
            let id: Uuid = row.get("id");
            sqlx::query("UPDATE devices SET name=$3,platform=$4,capabilities=$5 WHERE tenant_id=$1 AND id=$2")
                .bind(panel.tenant_id.as_str()).bind(id).bind(&device.name).bind(&device.platform).bind(serde_json::json!(device.capabilities)).execute(&mut *tx).await?;
            id
        } else {
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM devices WHERE tenant_id=$1 AND account_id=$2 AND revoked_at IS NULL")
                .bind(panel.tenant_id.as_str()).bind(account_id).fetch_one(&mut *tx).await?;
            let effective = entitlement_device_limit
                .filter(|n| *n > 0)
                .map_or(i64::from(limit), |n| i64::from(n).min(i64::from(limit)));
            if count >= effective {
                return Err(StoreError::DeviceLimit);
            }
            let total: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM devices WHERE tenant_id=$1 AND revoked_at IS NULL",
            )
            .bind(panel.tenant_id.as_str())
            .fetch_one(&mut *tx)
            .await?;
            if max_devices.is_some_and(|max| total >= max) {
                return Err(StoreError::DeviceLimit);
            }
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO devices(id,tenant_id,account_id,installation_hash,name,platform,capabilities) VALUES($1,$2,$3,$4,$5,$6,$7)")
                .bind(id).bind(panel.tenant_id.as_str()).bind(account_id).bind(installation_hash).bind(&device.name).bind(&device.platform).bind(serde_json::json!(device.capabilities)).execute(&mut *tx).await?;
            id
        };
        sqlx::query("UPDATE sessions SET revoked_at=$3 WHERE tenant_id=$1 AND device_id=$2 AND revoked_at IS NULL")
            .bind(panel.tenant_id.as_str()).bind(device_id).bind(now).execute(&mut *tx).await?;
        let access = generate_token(&panel.tenant_id, TokenKind::Access)
            .map_err(|_| StoreError::Unavailable)?;
        let refresh = generate_token(&panel.tenant_id, TokenKind::Refresh)
            .map_err(|_| StoreError::Unavailable)?;
        let session_id = Uuid::new_v4();
        sqlx::query("INSERT INTO sessions(id,tenant_id,account_id,device_id,access_token_hash,access_expires_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(session_id).bind(panel.tenant_id.as_str()).bind(account_id).bind(device_id).bind(access.hash.as_slice()).bind(now+Duration::minutes(15)).bind(now+Duration::days(30)).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO refresh_tokens(id,tenant_id,session_id,token_hash,expires_at) VALUES($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4()).bind(panel.tenant_id.as_str()).bind(session_id).bind(refresh.hash.as_slice()).bind(now+Duration::days(30)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(TokenPair {
            access_token: access.plaintext.expose().into(),
            refresh_token: refresh.plaintext.expose().into(),
            expires_in: 900,
            account_id,
            device_id,
            tenant_id: panel.tenant_id.as_str().into(),
        })
    }

    pub async fn authenticate(&self, token: &str) -> Result<Principal, StoreError> {
        let (tenant, hash) =
            inspect_token(token, TokenKind::Access).map_err(|_| StoreError::Unauthorized)?;
        let mut tx = self.begin(&tenant).await?;
        let row=sqlx::query("SELECT s.id,s.account_id,s.device_id,d.platform FROM sessions s JOIN devices d ON d.tenant_id=s.tenant_id AND d.id=s.device_id JOIN managed_accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id WHERE s.tenant_id=$1 AND s.access_token_hash=$2 AND s.revoked_at IS NULL AND s.access_expires_at>now() AND s.expires_at>now() AND d.revoked_at IS NULL AND a.status='active'")
            .bind(tenant.as_str()).bind(hash.as_slice()).fetch_optional(&mut *tx).await?.ok_or(StoreError::Unauthorized)?;
        Self::license(&mut tx, &tenant, row.get("platform"), Utc::now()).await?;
        tx.commit().await?;
        Ok(Principal {
            tenant_id: tenant,
            session_id: row.get("id"),
            account_id: row.get("account_id"),
            device_id: row.get("device_id"),
        })
    }

    pub async fn refresh(&self, token: &str) -> Result<TokenPair, StoreError> {
        let (tenant, hash) =
            inspect_token(token, TokenKind::Refresh).map_err(|_| StoreError::Unauthorized)?;
        let mut tx = self.begin(&tenant).await?;
        let row=sqlx::query("SELECT s.account_id,s.device_id,s.expires_at,d.platform FROM refresh_tokens r JOIN sessions s ON s.tenant_id=r.tenant_id AND s.id=r.session_id JOIN devices d ON d.tenant_id=s.tenant_id AND d.id=s.device_id WHERE r.tenant_id=$1 AND r.token_hash=$2")
            .bind(tenant.as_str()).bind(hash.as_slice()).fetch_optional(&mut *tx).await?.ok_or(StoreError::Unauthorized)?;
        Self::license(&mut tx, &tenant, row.get("platform"), Utc::now()).await?;
        let access =
            generate_token(&tenant, TokenKind::Access).map_err(|_| StoreError::Unavailable)?;
        let refresh =
            generate_token(&tenant, TokenKind::Refresh).map_err(|_| StoreError::Unavailable)?;
        let result: String = sqlx::query_scalar("SELECT rotate_refresh_token($1,$2,$3,$4,now())")
            .bind(hash.as_slice())
            .bind(access.hash.as_slice())
            .bind(Uuid::new_v4())
            .bind(refresh.hash.as_slice())
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        if result != "ok" {
            return Err(StoreError::Unauthorized);
        }
        Ok(TokenPair {
            access_token: access.plaintext.expose().into(),
            refresh_token: refresh.plaintext.expose().into(),
            expires_in: (row.get::<DateTime<Utc>, _>("expires_at") - Utc::now())
                .num_seconds()
                .clamp(0, 900),
            tenant_id: tenant.as_str().into(),
            account_id: row.get("account_id"),
            device_id: row.get("device_id"),
        })
    }

    pub async fn logout(&self, actor: &Principal) -> Result<(), StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        sqlx::query(
            "UPDATE sessions SET revoked_at=now() WHERE tenant_id=$1 AND id=$2 AND account_id=$3",
        )
        .bind(actor.tenant_id.as_str())
        .bind(actor.session_id)
        .bind(actor.account_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn devices(&self, actor: &Principal) -> Result<Vec<DeviceView>, StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let rows=sqlx::query("SELECT id,name,platform,revoked_at FROM devices WHERE tenant_id=$1 AND account_id=$2 ORDER BY created_at")
            .bind(actor.tenant_id.as_str()).bind(actor.account_id).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(rows
            .iter()
            .map(|r| DeviceView {
                id: r.get("id"),
                name: r.get("name"),
                platform: r.get("platform"),
                revoked_at: r.get("revoked_at"),
            })
            .collect())
    }

    pub async fn revoke_device(&self, actor: &Principal, id: Uuid) -> Result<(), StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let result = sqlx::query(
            "UPDATE devices SET revoked_at=now() WHERE tenant_id=$1 AND account_id=$2 AND id=$3",
        )
        .bind(actor.tenant_id.as_str())
        .bind(actor.account_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        sqlx::query("UPDATE sessions SET revoked_at=now() WHERE tenant_id=$1 AND device_id=$2")
            .bind(actor.tenant_id.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn panel_session(&self, actor: &Principal) -> Result<PanelSession, StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let row=sqlx::query("SELECT external_user_id,panel_credential_encrypted FROM managed_accounts WHERE tenant_id=$1 AND id=$2 AND status='active'")
            .bind(actor.tenant_id.as_str()).bind(actor.account_id).fetch_optional(&mut *tx).await?.ok_or(StoreError::Unauthorized)?;
        let data: Vec<u8> = row.get("panel_credential_encrypted");
        let plain = self
            .key
            .decrypt(
                &data,
                format!(
                    "panel_credential:{}:{}",
                    actor.tenant_id.as_str(),
                    actor.account_id
                )
                .as_bytes(),
            )
            .map_err(|_| StoreError::Unavailable)?;
        let credential = Secret::new(
            std::str::from_utf8(&plain)
                .map_err(|_| StoreError::Unavailable)?
                .into(),
        );
        tx.commit().await?;
        Ok(PanelSession {
            tenant_id: actor.tenant_id.clone(),
            external_user_id: row.get("external_user_id"),
            credential,
        })
    }
}
