use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

fn tid() -> TenantId {
    TenantId::parse("sample").unwrap()
}

async fn server(replies: Vec<(u16, &'static str)>) -> (XBoardAdapter, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 2048];
                let n = stream.read(&mut buffer).await.unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
                assert!(bytes.len() < 100_000);
            }
            requests.push(String::from_utf8(bytes).unwrap());
            let content_type = if body.starts_with("<html") {
                "text/html"
            } else {
                "application/json"
            };
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });
    let adapter = XBoardAdapter {
        tenant_id: tid(),
        base: Url::parse(&format!("http://{address}")).unwrap(),
        client: Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap(),
    };
    (adapter, task)
}

const USER: &str = r#"{"data":{"id":42,"email":"test@example.com","banned":false}}"#;
const SUB: &str = r#"{"data":{"token":"subscription-token","subscribe_url":"https://untrusted.invalid/secret","expired_at":null,"transfer_enable":"1024","u":1,"d":"2","device_limit":2}}"#;

fn credentials() -> Credentials {
    Credentials {
        email: "test@example.com".into(),
        password: Secret::new("test-password".into()),
    }
}

fn session() -> PanelSession {
    PanelSession {
        tenant_id: tid(),
        external_user_id: "42".into(),
        credential: Secret::new("Bearer test-token".into()),
    }
}

#[tokio::test]
async fn login_fetches_stable_identity_and_preserves_bearer_format() {
    let (adapter, task) = server(vec![
        (200, r#"{"data":{"auth_data":"Bearer test-token"}}"#),
        (200, USER),
    ])
    .await;
    let session = adapter.authenticate(&credentials()).await.unwrap();
    assert_eq!(session.external_user_id, "42");
    let requests = task.await.unwrap();
    assert!(requests[0].starts_with("POST /api/v1/passport/auth/login "));
    assert!(requests[0].contains("test-password"));
    assert!(requests[1].contains("authorization: Bearer test-token\r\n"));
    assert!(!format!("{session:?}").contains("test-token"));
}

#[tokio::test]
async fn missing_numeric_id_does_not_fall_back_to_resettable_uuid() {
    let (adapter, task) = server(vec![
        (200, r#"{"data":{"auth_data":"Bearer test-token"}}"#),
        (
            200,
            r#"{"data":{"uuid":"resettable","email":"test@example.com","banned":false}}"#,
        ),
    ])
    .await;
    assert!(matches!(
        adapter.authenticate(&credentials()).await,
        Err(PanelError::StableIdentityUnavailable)
    ));
    task.await.unwrap();
}

#[tokio::test]
async fn account_change_cannot_rebind_an_existing_session() {
    let (adapter, task) = server(vec![(200, USER)]).await;
    let mut session = session();
    session.external_user_id = "77".into();
    assert_eq!(
        adapter.get_user(&session).await,
        Err(PanelError::IdentityMismatch)
    );
    task.await.unwrap();
}

#[tokio::test]
async fn entitlement_normalizes_numeric_strings() {
    let (adapter, task) = server(vec![(200, USER), (200, SUB)]).await;
    let value = adapter.get_entitlement(&session()).await.unwrap();
    assert_eq!(value.quota_bytes, 1024);
    assert_eq!(value.download_bytes, 2);
    assert_eq!(value.device_limit, Some(2));
    assert!(value.usable_at(100));
    task.await.unwrap();
}

#[tokio::test]
async fn profile_uses_pinned_panel_route_not_returned_subscription_url() {
    let (adapter, task) = server(vec![
        (200, USER),
        (200, SUB),
        (200, "proxies:\n  - name: demo\n    type: direct\n"),
    ])
    .await;
    let profile = adapter.get_subscription(&session()).await.unwrap();
    assert!(profile.etag().starts_with('"'));
    let requests = task.await.unwrap();
    assert!(
        requests[2]
            .starts_with("GET /api/v1/client/subscribe?token=subscription-token&flag=clash.meta ")
    );
    assert!(!requests[2].contains("authorization:"));
}

#[tokio::test]
async fn session_and_entitlement_denials_are_distinct() {
    let (adapter, task) = server(vec![(403, "secret server message")]).await;
    assert_eq!(
        adapter.get_user(&session()).await,
        Err(PanelError::SessionExpired)
    );
    task.await.unwrap();
    let (adapter, task) = server(vec![(200, USER), (200, SUB), (403, "")]).await;
    assert!(matches!(
        adapter.get_subscription(&session()).await,
        Err(PanelError::EntitlementDenied)
    ));
    task.await.unwrap();
}

#[tokio::test]
async fn foreign_tenant_is_rejected_without_network_request() {
    let adapter = XBoardAdapter::new(tid(), "https://example.invalid", None).unwrap();
    let mut session = session();
    session.tenant_id = TenantId::parse("foreign").unwrap();
    assert_eq!(
        adapter.get_user(&session).await,
        Err(PanelError::TenantMismatch)
    );
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let (adapter, task) = server(vec![(302, "redirect")]).await;
    assert_eq!(
        adapter.get_user(&session()).await,
        Err(PanelError::InvalidResponse)
    );
    task.await.unwrap();
}

#[test]
fn configured_panel_url_requires_clean_https_origin() {
    for url in [
        "http://example.com",
        "https://user:pass@example.com",
        "https://example.com?q=x",
        "https://example.com/#fragment",
        "https://example.com/path",
    ] {
        assert!(matches!(
            XBoardAdapter::new(tid(), url, None),
            Err(PanelError::InvalidConfiguration)
        ));
    }
}

#[test]
fn malformed_entitlement_is_not_unlimited() {
    for data in [
        serde_json::json!({}),
        serde_json::json!({"expired_at":null,"transfer_enable":-1,"u":0,"d":0,"device_limit":2}),
    ] {
        assert_eq!(entitlement(&data), Err(PanelError::InvalidResponse));
    }
}

#[tokio::test]
async fn edge_block_page_is_not_treated_as_account_revocation() {
    let (adapter, task) = server(vec![(403, "<html>edge challenge</html>")]).await;
    assert_eq!(
        adapter.get_user(&session()).await,
        Err(PanelError::Unavailable)
    );
    task.await.unwrap();
}

#[tokio::test]
async fn rate_limit_and_server_failure_remain_distinct() {
    for (status, expected) in [
        (429, PanelError::RateLimited),
        (503, PanelError::Unavailable),
    ] {
        let (adapter, task) = server(vec![(status, "{}")]).await;
        assert_eq!(adapter.get_user(&session()).await, Err(expected));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn response_size_limit_is_enforced_before_json_parsing() {
    let (adapter, task) = server(vec![(200, USER)]).await;
    let request = adapter.request(Method::GET, "/api/v1/user/info").unwrap();
    assert_eq!(
        XBoardAdapter::bytes(request, Operation::Account, 4).await,
        Err(PanelError::ResponseTooLarge)
    );
    task.await.unwrap();
}

#[tokio::test]
async fn invalid_authorization_header_never_reaches_network() {
    let adapter = XBoardAdapter::new(tid(), "https://example.invalid", None).unwrap();
    let mut session = session();
    session.credential = Secret::new("Bearer token\r\nX-Injected: yes".into());
    assert_eq!(
        adapter.get_user(&session).await,
        Err(PanelError::SessionExpired)
    );
}
