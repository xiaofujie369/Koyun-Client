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
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

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
    for name in ["demo", "other"] {
        registry
            .register(Arc::new(MockAdapter::new(TenantId::parse(name).unwrap())))
            .unwrap();
    }
    let app = router(ApiState {
        store,
        adapters: Arc::new(registry),
    });
    let device = Uuid::new_v4();
    let (status, result) = call(&app, "POST", "/v1/auth/login", None, login("demo", device)).await;
    assert_eq!(status, StatusCode::OK, "login returned {result}");
    let access = result["data"]["access_token"].as_str().unwrap();
    let refresh = result["data"]["refresh_token"].as_str().unwrap();
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
}
