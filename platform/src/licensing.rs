use crate::scope::TenantId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseStatus {
    Trial,
    Active,
    Grace,
    Suspended,
    Expired,
    Revoked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientPlatform {
    Linux,
    Windows,
    Android,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedCapability {
    Synchronize,
    Realtime,
    WhiteLabel,
    CustomDomain,
}

#[derive(Clone, Debug)]
pub struct TenantLicense {
    pub tenant_id: TenantId,
    pub status: LicenseStatus,
    pub starts_at: u64,
    pub expires_at: Option<u64>,
    pub grace_until: Option<u64>,
    pub platforms: Vec<ClientPlatform>,
    pub capabilities: Vec<ManagedCapability>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessDenied {
    TenantMismatch,
    TenantInactive,
    NotStarted,
    LicenseInactive,
    LicenseExpired,
    PlatformDisabled,
    CapabilityDisabled,
}

pub struct TenantLicenseService;

impl TenantLicenseService {
    pub fn authorize(
        tenant_id: &TenantId,
        tenant_active: bool,
        license: &TenantLicense,
        platform: ClientPlatform,
        capability: ManagedCapability,
        now: u64,
    ) -> Result<(), AccessDenied> {
        if license.tenant_id != *tenant_id {
            return Err(AccessDenied::TenantMismatch);
        }
        if !tenant_active {
            return Err(AccessDenied::TenantInactive);
        }
        if now < license.starts_at {
            return Err(AccessDenied::NotStarted);
        }
        match license.status {
            LicenseStatus::Suspended | LicenseStatus::Expired | LicenseStatus::Revoked => {
                return Err(AccessDenied::LicenseInactive);
            }
            LicenseStatus::Grace => {
                if license.grace_until.is_none_or(|end| now >= end) {
                    return Err(AccessDenied::LicenseExpired);
                }
            }
            LicenseStatus::Trial | LicenseStatus::Active => {
                if license.expires_at.is_some_and(|end| now >= end) {
                    return Err(AccessDenied::LicenseExpired);
                }
            }
        }
        if !license.platforms.contains(&platform) {
            return Err(AccessDenied::PlatformDisabled);
        }
        if !license.capabilities.contains(&capability) {
            return Err(AccessDenied::CapabilityDisabled);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteState {
    Authorized,
    Unreachable,
    AccountDisabled,
    DeviceRevoked,
    EntitlementExpired,
    LicenseDenied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachedProfileDecision {
    UseLastKnownGood,
    NoValidatedProfile,
    ReauthorizationRequired,
    ExplicitlyDenied,
}

pub fn cached_profile_decision(
    state: RemoteState,
    has_validated_profile: bool,
    last_authorized_at: Option<u64>,
    offline_grace_seconds: u64,
    now: u64,
) -> CachedProfileDecision {
    if matches!(
        state,
        RemoteState::AccountDisabled
            | RemoteState::DeviceRevoked
            | RemoteState::EntitlementExpired
            | RemoteState::LicenseDenied
    ) {
        return CachedProfileDecision::ExplicitlyDenied;
    }
    if !has_validated_profile {
        return CachedProfileDecision::NoValidatedProfile;
    }
    if state == RemoteState::Authorized {
        return CachedProfileDecision::UseLastKnownGood;
    }
    match last_authorized_at.and_then(|last| now.checked_sub(last)) {
        Some(elapsed) if elapsed < offline_grace_seconds => CachedProfileDecision::UseLastKnownGood,
        _ => CachedProfileDecision::ReauthorizationRequired,
    }
}
