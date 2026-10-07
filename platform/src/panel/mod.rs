mod mock;
mod profile;
mod registry;
mod xboard;

pub use mock::MockAdapter;
pub use profile::ProfileDocument;
pub use registry::AdapterRegistry;
pub use xboard::XBoardAdapter;

use crate::scope::TenantId;
use async_trait::async_trait;
use std::fmt;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

pub struct Credentials {
    pub email: String,
    pub password: Secret,
}

#[derive(Clone, Debug)]
pub struct PanelSession {
    pub tenant_id: TenantId,
    pub external_user_id: String,
    pub credential: Secret,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelUser {
    pub external_user_id: String,
    pub email: String,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entitlement {
    pub expires_at: Option<u64>,
    pub quota_bytes: u64,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub device_limit: Option<u32>,
}

impl Entitlement {
    pub fn usable_at(&self, now: u64) -> bool {
        self.expires_at.is_none_or(|end| now < end)
            && self.upload_bytes.saturating_add(self.download_bytes) < self.quota_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelError {
    InvalidConfiguration,
    UnknownTenant,
    DuplicateTenant,
    TenantMismatch,
    IdentityMismatch,
    StableIdentityUnavailable,
    InvalidCredentials,
    SessionExpired,
    AccountDisabled,
    EntitlementDenied,
    RateLimited,
    Unavailable,
    InvalidResponse,
    ResponseTooLarge,
    InvalidProfile,
}

impl fmt::Display for PanelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PanelError {}

#[async_trait]
pub trait PanelAdapter: Send + Sync {
    fn tenant_id(&self) -> &TenantId;
    async fn authenticate(&self, credentials: &Credentials) -> Result<PanelSession, PanelError>;
    async fn get_user(&self, session: &PanelSession) -> Result<PanelUser, PanelError>;
    async fn get_entitlement(&self, session: &PanelSession) -> Result<Entitlement, PanelError>;
    async fn get_subscription(&self, session: &PanelSession)
    -> Result<ProfileDocument, PanelError>;
    async fn refresh_subscription(
        &self,
        session: &PanelSession,
    ) -> Result<ProfileDocument, PanelError> {
        self.get_subscription(session).await
    }
}
