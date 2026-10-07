use client_platform::licensing::*;
use client_platform::scope::*;

fn license(tenant: &str) -> TenantLicense {
    TenantLicense {
        tenant_id: TenantId::parse(tenant).unwrap(),
        status: LicenseStatus::Active,
        starts_at: 100,
        expires_at: Some(200),
        grace_until: None,
        platforms: vec![ClientPlatform::Linux],
        capabilities: vec![ManagedCapability::Synchronize],
    }
}

fn access(tenant: &str, license: &TenantLicense, now: u64) -> Result<(), AccessDenied> {
    TenantLicenseService::authorize(
        &TenantId::parse(tenant).unwrap(),
        true,
        license,
        ClientPlatform::Linux,
        ManagedCapability::Synchronize,
        now,
    )
}

#[test]
fn license_cannot_authorize_another_tenant() {
    assert_eq!(
        access("beta", &license("alpha"), 150),
        Err(AccessDenied::TenantMismatch)
    );
}

#[test]
fn suspending_one_tenant_does_not_suspend_another() {
    let mut alpha = license("alpha");
    let beta = license("beta");
    alpha.status = LicenseStatus::Suspended;
    assert_eq!(
        access("alpha", &alpha, 150),
        Err(AccessDenied::LicenseInactive)
    );
    assert_eq!(access("beta", &beta, 150), Ok(()));
}

#[test]
fn validity_interval_excludes_expiry_instant() {
    let value = license("alpha");
    assert_eq!(access("alpha", &value, 99), Err(AccessDenied::NotStarted));
    assert_eq!(access("alpha", &value, 100), Ok(()));
    assert_eq!(access("alpha", &value, 199), Ok(()));
    assert_eq!(
        access("alpha", &value, 200),
        Err(AccessDenied::LicenseExpired)
    );
}

#[test]
fn grace_requires_explicit_unexpired_deadline() {
    let mut value = license("alpha");
    value.status = LicenseStatus::Grace;
    assert_eq!(
        access("alpha", &value, 201),
        Err(AccessDenied::LicenseExpired)
    );
    value.grace_until = Some(220);
    assert_eq!(access("alpha", &value, 219), Ok(()));
    assert_eq!(
        access("alpha", &value, 220),
        Err(AccessDenied::LicenseExpired)
    );
}

#[test]
fn authenticated_scope_checks_tenant_and_account() {
    let scope =
        |t: &str, a: &str| AccountScope::new(TenantId::parse(t).unwrap(), a.into()).unwrap();
    let actor = scope("alpha", "one");
    assert_eq!(
        actor.require_owner(&scope("beta", "one")),
        Err(ScopeError::TenantMismatch)
    );
    assert_eq!(
        actor.require_owner(&scope("alpha", "two")),
        Err(ScopeError::AccountMismatch)
    );
    assert_eq!(actor.require_owner(&scope("alpha", "one")), Ok(()));
}

#[test]
fn explicit_denial_never_enters_offline_grace() {
    for state in [
        RemoteState::DeviceRevoked,
        RemoteState::AccountDisabled,
        RemoteState::EntitlementExpired,
        RemoteState::LicenseDenied,
    ] {
        assert_eq!(
            cached_profile_decision(state, true, Some(100), 1000, 101),
            CachedProfileDecision::ExplicitlyDenied
        );
    }
}

#[test]
fn grace_requires_lkg_and_previous_authorization_and_monotonic_time() {
    let decision =
        |valid, last, now| cached_profile_decision(RemoteState::Unreachable, valid, last, 10, now);
    assert_eq!(
        decision(false, Some(100), 101),
        CachedProfileDecision::NoValidatedProfile
    );
    assert_eq!(
        decision(true, None, 101),
        CachedProfileDecision::ReauthorizationRequired
    );
    assert_eq!(
        decision(true, Some(100), 99),
        CachedProfileDecision::ReauthorizationRequired
    );
    assert_eq!(
        decision(true, Some(100), 109),
        CachedProfileDecision::UseLastKnownGood
    );
    assert_eq!(
        decision(true, Some(100), 110),
        CachedProfileDecision::ReauthorizationRequired
    );
}

#[test]
fn feature_and_platform_permissions_are_independent() {
    let value = license("alpha");
    let check = |active, platform, capability| {
        TenantLicenseService::authorize(&value.tenant_id, active, &value, platform, capability, 150)
    };
    assert_eq!(
        check(false, ClientPlatform::Linux, ManagedCapability::Synchronize),
        Err(AccessDenied::TenantInactive)
    );
    assert_eq!(
        check(
            true,
            ClientPlatform::Windows,
            ManagedCapability::Synchronize
        ),
        Err(AccessDenied::PlatformDisabled)
    );
    assert_eq!(
        check(true, ClientPlatform::Linux, ManagedCapability::Realtime),
        Err(AccessDenied::CapabilityDisabled)
    );
}

#[test]
fn tenant_identifiers_reject_path_and_empty_values() {
    for value in ["", "../alpha", "alpha/beta", "alpha\\beta", "alpha beta"] {
        assert_eq!(TenantId::parse(value), Err(ScopeError::InvalidIdentifier));
    }
}
