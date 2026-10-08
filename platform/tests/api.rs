use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use client_platform::{
    api::{ApiState, router},
    crypto::MasterKey,
    panel::{AdapterRegistry, MockAdapter},
    scope::TenantId,
    store::Store,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use tower::ServiceExt;
use uuid::Uuid;

struct ControlledAdapter {
    inner: MockAdapter,
    mode: Arc<std::sync::atomic::AtomicU8>,
}
#[async_trait::async_trait]
impl client_platform::panel::PanelAdapter for ControlledAdapter {
    fn tenant_id(&self) -> &TenantId {
        client_platform::panel::PanelAdapter::tenant_id(&self.inner)
    }
    async fn authenticate(
        &self,
        credentials: &client_platform::panel::Credentials,
    ) -> Result<client_platform::panel::PanelSession, client_platform::panel::PanelError> {
        self.inner.authenticate(credentials).await
    }
    async fn get_user(
        &self,
        session: &client_platform::panel::PanelSession,
    ) -> Result<client_platform::panel::PanelUser, client_platform::panel::PanelError> {
        self.inner.get_user(session).await
    }
    async fn get_entitlement(
        &self,
        session: &client_platform::panel::PanelSession,
    ) -> Result<client_platform::panel::Entitlement, client_platform::panel::PanelError> {
        if self.mode.load(std::sync::atomic::Ordering::SeqCst) == 2 {
            return Err(client_platform::panel::PanelError::EntitlementDenied);
        }
        self.inner.get_entitlement(session).await
    }
    async fn get_subscription(
        &self,
        session: &client_platform::panel::PanelSession,
    ) -> Result<client_platform::panel::ProfileDocument, client_platform::panel::PanelError> {
        match self.mode.load(std::sync::atomic::Ordering::SeqCst) {
            1 => Err(client_platform::panel::PanelError::Unavailable),
            3 => Err(client_platform::panel::PanelError::InvalidProfile),
            _ => self.inner.get_subscription(session).await,
        }
    }
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let request = request.extension(axum::extract::ConnectInfo(
        "127.0.0.1:10000".parse::<std::net::SocketAddr>().unwrap(),
    ));
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn login(tenant: &str, id: Uuid) -> Value {
    json!({"tenant_id":tenant,"email":"demo@example.com","password":"demo","device":{"installation_id":id,"name":"Integration Linux","platform":"linux"}})
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database initialized with api_setup.sql"]
async fn session_rotation_device_quota_and_tenant_isolation() {
    let pool = sqlx::PgPool::connect(
        &std::env::var("TEST_RUNTIME_DATABASE_URL").expect("test database URL required"),
    )
    .await
    .unwrap();
    let store = Arc::new(
        Store::new(pool.clone(), MasterKey::from_hex(&"11".repeat(32)).unwrap())
            .await
            .unwrap(),
    );
    let mut registry = AdapterRegistry::default();
    let fault_mode = Arc::new(std::sync::atomic::AtomicU8::new(0));
    for name in ["demo", "other"] {
        registry
            .register(Arc::new(ControlledAdapter {
                inner: MockAdapter::new(TenantId::parse(name).unwrap()),
                mode: if name == "demo" {
                    fault_mode.clone()
                } else {
                    Arc::new(std::sync::atomic::AtomicU8::new(0))
                },
            }))
            .unwrap();
    }
    let app = router(ApiState {
        store: store.clone(),
        adapters: Arc::new(registry),
    });
    let device = Uuid::new_v4();
    let (status, result) = call(&app, "POST", "/v1/auth/login", None, login("demo", device)).await;
    assert_eq!(status, StatusCode::OK, "login returned {result}");
    let access = result["data"]["access_token"].as_str().unwrap();
    let refresh = result["data"]["refresh_token"].as_str().unwrap();
    let (status, profiles) = call(
        &app,
        "GET",
        "/v1/managed/profiles",
        Some(access),
        Value::Null,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "profile listing returned {profiles}"
    );
    let profile_id = profiles["data"][0]["id"].as_str().unwrap();
    let profile_etag = profiles["data"][0]["etag"].as_str().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/managed/profiles/{profile_id}"))
                .header("authorization", format!("Bearer {access}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["etag"], profile_etag);
    assert!(
        String::from_utf8(
            to_bytes(response.into_body(), 65536)
                .await
                .unwrap()
                .to_vec()
        )
        .unwrap()
        .contains("proxies:")
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/managed/profiles/{profile_id}"))
                .header("authorization", format!("Bearer {access}"))
                .header("if-none-match", profile_etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        call(&app, "GET", "/v1/sync/state", Some(access), Value::Null)
            .await
            .1["data"]["profile_version"],
        1
    );
    let principal = store.authenticate(access).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id','demo',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE managed_profiles SET last_sync_at=now()-interval '1 minute' WHERE tenant_id='demo'",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for mode in [1, 3] {
        fault_mode.store(mode, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            call(
                &app,
                "GET",
                "/v1/managed/profiles",
                Some(access),
                Value::Null
            )
            .await
            .0,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            store
                .managed_content(&principal, Uuid::parse_str(profile_id).unwrap())
                .await
                .unwrap()
                .0
                .version,
            1
        );
    }
    fault_mode.store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        call(
            &app,
            "GET",
            "/v1/managed/profiles",
            Some(access),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(matches!(
        store
            .managed_content(&principal, Uuid::parse_str(profile_id).unwrap())
            .await,
        Err(client_platform::store::StoreError::Forbidden)
    ));
    fault_mode.store(0, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        call(
            &app,
            "GET",
            "/v1/managed/profiles",
            Some(access),
            Value::Null
        )
        .await
        .1["data"][0]["version"],
        3
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/v1/tenants/other/me",
            Some(access),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/v1/tenants/demo/me",
            Some(access),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, rotated) = call(
        &app,
        "POST",
        "/v1/auth/refresh",
        None,
        json!({"refresh_token":refresh}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&app, "GET", "/v1/devices", Some(access), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let new_access = rotated["data"]["access_token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", "/v1/devices", Some(new_access), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/refresh",
            None,
            json!({"refresh_token":refresh})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", "/v1/devices", Some(new_access), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/refresh",
            None,
            json!({"refresh_token":rotated["data"]["refresh_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (first, second) = tokio::join!(
        call(
            &app,
            "POST",
            "/v1/auth/login",
            None,
            login("demo", Uuid::new_v4())
        ),
        call(
            &app,
            "POST",
            "/v1/auth/login",
            None,
            login("demo", Uuid::new_v4())
        )
    );
    let mut statuses = [first.0.as_u16(), second.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let winner = if first.0 == StatusCode::OK {
        first.1
    } else {
        second.1
    };
    let access = winner["data"]["access_token"].as_str().unwrap();
    let (status, other) = call(
        &app,
        "POST",
        "/v1/auth/login",
        None,
        login("other", Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let foreign = other["data"]["device_id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/v1/devices/{foreign}"),
            Some(access),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let id = winner["data"]["device_id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/v1/devices/{id}"),
            Some(access),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/v1/devices", Some(access), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let other_access = other["data"]["access_token"].as_str().unwrap();
    let principal = store.authenticate(other_access).await.unwrap();
    let ticket = store.realtime_ticket(&principal).await.unwrap();
    let (one, two) = tokio::join!(
        store.consume_realtime_ticket(&ticket),
        store.consume_realtime_ticket(&ticket)
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let ticket = store.realtime_ticket(&principal).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            server_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut request = format!("ws://{address}/v1/realtime/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {ticket}").parse().unwrap());
    let (mut websocket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let message = tokio::time::timeout(std::time::Duration::from_secs(5), websocket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&message.into_text().unwrap()).unwrap()["type"],
        "sync.required"
    );
    assert!(store.consume_realtime_ticket(&ticket).await.is_err());
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/v1/managed/profiles/{profile_id}"),
            Some(other_access),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, other_profiles) = call(
        &app,
        "GET",
        "/v1/managed/profiles",
        Some(other_access),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let principal = store.authenticate(other_access).await.unwrap();
    let changed=client_platform::panel::ProfileDocument::parse(b"proxies:\n  - {name: Updated, type: ss, server: 192.0.2.1, port: 443, cipher: aes-128-gcm, password: test-only}\n".to_vec()).unwrap();
    let before = chrono::Utc::now() - chrono::Duration::minutes(1);
    let updated = store
        .cache_profile(&principal, &changed, chrono::Utc::now())
        .await
        .unwrap();
    assert_eq!(updated.version, 2);
    let stale = store
        .cache_profile(&principal, &changed, before)
        .await
        .unwrap();
    assert_eq!(stale.version, 2);
    let same = store
        .cache_profile(&principal, &changed, chrono::Utc::now())
        .await
        .unwrap();
    assert_eq!(same.version, 2);
    store.suspend_profile(&principal).await.unwrap();
    let stale = store
        .cache_profile(&principal, &changed, before)
        .await
        .unwrap();
    assert_eq!(stale.status, "suspended");
    let other_id = Uuid::parse_str(other_profiles["data"][0]["id"].as_str().unwrap()).unwrap();
    assert!(matches!(
        store.managed_content(&principal, other_id).await,
        Err(client_platform::store::StoreError::Forbidden)
    ));
    let (status, registered) = call(
        &app,
        "POST",
        "/v1/devices/register",
        Some(other_access),
        login("other", Uuid::new_v4())["device"].clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        registered["data"]["account_id"],
        other["data"]["account_id"]
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/devices/register",
            Some(other_access),
            login("other", Uuid::new_v4())["device"].clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id','other',true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    let encrypted: Vec<u8> = sqlx::query_scalar(
        "SELECT panel_credential_encrypted FROM managed_accounts WHERE tenant_id='other'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(encrypted.len() > 29);
    let account_id = other["data"]["account_id"].as_str().unwrap();
    let decrypted = MasterKey::from_hex(&"11".repeat(32))
        .unwrap()
        .decrypt(
            &encrypted,
            format!("panel_credential:other:{account_id}").as_bytes(),
        )
        .unwrap();
    assert_eq!(decrypted.as_slice(), b"demo-session");
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/logout",
            Some(other_access),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/refresh",
            None,
            json!({"refresh_token":other["data"]["refresh_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM managed_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "pooled connections must not retain tenant scope");
    tokio::time::timeout(std::time::Duration::from_secs(6), async {
        loop {
            if matches!(
                websocket.next().await,
                None | Some(Err(_)) | Some(Ok(Message::Close(_)))
            ) {
                break;
            }
        }
    })
    .await
    .expect("logout must disconnect the authenticated WebSocket");
    server.abort();
}
