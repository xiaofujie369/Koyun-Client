use client_platform::{panel::*, scope::TenantId};
use std::sync::Arc;

fn tid(value: &str) -> TenantId {
    TenantId::parse(value).unwrap()
}

#[tokio::test]
async fn demo_and_another_provider_have_independent_sessions() {
    let mut registry = AdapterRegistry::default();
    registry
        .register(Arc::new(MockAdapter::new(tid("tenant_demo"))))
        .unwrap();
    registry
        .register(Arc::new(MockAdapter::new(tid("other"))))
        .unwrap();
    let demo = registry.get(&tid("tenant_demo")).unwrap();
    let other = registry.get(&tid("other")).unwrap();
    let session = demo
        .authenticate(&Credentials {
            email: "demo@example.com".into(),
            password: Secret::new("demo".into()),
        })
        .await
        .unwrap();
    assert!(demo.get_entitlement(&session).await.unwrap().usable_at(100));
    assert!(demo.get_subscription(&session).await.is_ok());
    assert_eq!(
        other.get_user(&session).await,
        Err(PanelError::TenantMismatch)
    );
    assert!(matches!(
        registry.get(&tid("missing")),
        Err(PanelError::UnknownTenant)
    ));
    assert_eq!(
        registry.register(Arc::new(MockAdapter::new(tid("tenant_demo")))),
        Err(PanelError::DuplicateTenant)
    );
}

#[tokio::test]
async fn demo_rejects_invalid_password_and_forged_session() {
    let adapter = MockAdapter::new(tid("demo"));
    assert!(matches!(
        adapter
            .authenticate(&Credentials {
                email: "demo@example.com".into(),
                password: Secret::new("wrong".into())
            })
            .await,
        Err(PanelError::InvalidCredentials)
    ));
    let session = PanelSession {
        tenant_id: tid("demo"),
        external_user_id: "demo-user".into(),
        credential: Secret::new("forged".into()),
    };
    assert_eq!(
        adapter.get_user(&session).await,
        Err(PanelError::SessionExpired)
    );
}

#[test]
fn profile_rejects_html_scalar_empty_and_oversized_responses() {
    for content in [
        b"<html>blocked</html>".to_vec(),
        b"true".to_vec(),
        b"proxies: []".to_vec(),
        b"proxies: [".to_vec(),
    ] {
        assert!(matches!(
            ProfileDocument::parse(content),
            Err(PanelError::InvalidProfile)
        ));
    }
    assert!(matches!(
        ProfileDocument::parse(vec![b' '; 8 * 1024 * 1024 + 1]),
        Err(PanelError::ResponseTooLarge)
    ));
}

#[test]
fn profile_etag_tracks_content_and_debug_redacts_secrets() {
    let first = ProfileDocument::parse(b"proxies: [{name: a, password: secret}]".to_vec()).unwrap();
    let same = ProfileDocument::parse(first.content().to_vec()).unwrap();
    let different =
        ProfileDocument::parse(b"proxies: [{name: b, password: secret}]".to_vec()).unwrap();
    assert_eq!(first.etag(), same.etag());
    assert_ne!(first.etag(), different.etag());
    assert!(!format!("{first:?}").contains("secret"));
}

#[test]
fn entitlement_boundaries_and_overflow_fail_closed() {
    let mut value = Entitlement {
        expires_at: Some(100),
        quota_bytes: 10,
        upload_bytes: 4,
        download_bytes: 5,
        device_limit: None,
    };
    assert!(value.usable_at(99));
    assert!(!value.usable_at(100));
    value.download_bytes = 6;
    assert!(!value.usable_at(99));
    value.download_bytes = u64::MAX;
    assert!(!value.usable_at(99));
}
