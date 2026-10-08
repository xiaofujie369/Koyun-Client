use crate::{
    panel::{AdapterRegistry, Credentials, PanelError, Secret},
    scope::TenantId,
    store::{DeviceInput, Principal, Store, StoreError},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub store: Arc<Store>,
    pub adapters: Arc<AdapterRegistry>,
    pub bootstrap: Option<Arc<crate::bootstrap::BootstrapConfig>>,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .route("/v1/bootstrap", get(crate::bootstrap::bootstrap))
        .route("/v1/public/brands/{id}", get(crate::bootstrap::brand))
        .route("/v1/public/tenants", get(tenants))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/refresh", post(refresh))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/register", post(register_device))
        .route("/v1/devices/{id}", delete(revoke))
        .route("/v1/tenants/{id}/me", get(me))
        .route("/v1/tenants/{id}/entitlement", get(entitlement))
        .route("/v1/managed/profiles", get(crate::api_managed::profiles))
        .route(
            "/v1/managed/profiles/{id}",
            get(crate::api_managed::content),
        )
        .route(
            "/v1/managed/profiles/{id}/state",
            get(crate::api_managed::profile_state),
        )
        .route("/v1/sync/state", get(crate::api_managed::sync_state))
        .route("/v1/realtime/token", get(crate::realtime::ticket))
        .route("/v1/realtime/ws", get(crate::realtime::upgrade))
        .route("/v1/app/policy", get(app_policy))
        .route("/v1/notices", get(notices))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(crate::api_limits::Limits::default()),
            crate::api_limits::guard,
        ))
        .layer(axum::middleware::map_response(
            |mut response: Response| async move {
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-store"),
                );
                response.headers_mut().insert(
                    "x-request-id",
                    Uuid::new_v4()
                        .to_string()
                        .parse()
                        .expect("UUID is a header value"),
                );
                response
            },
        ))
        .with_state(state)
}

pub struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":{"code":self.1}}))).into_response()
    }
}
impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        match value {
            StoreError::Unavailable => Self(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable"),
            StoreError::Unauthorized => Self(StatusCode::UNAUTHORIZED, "unauthorized"),
            StoreError::Forbidden => Self(StatusCode::FORBIDDEN, "access_denied"),
            StoreError::DeviceLimit => Self(StatusCode::CONFLICT, "device_limit"),
            StoreError::InvalidInput => Self(StatusCode::BAD_REQUEST, "invalid_input"),
            StoreError::NotFound => Self(StatusCode::NOT_FOUND, "not_found"),
        }
    }
}
impl From<PanelError> for ApiError {
    fn from(value: PanelError) -> Self {
        match value {
            PanelError::InvalidCredentials | PanelError::SessionExpired => {
                Self(StatusCode::UNAUTHORIZED, "panel_authentication_failed")
            }
            PanelError::AccountDisabled | PanelError::EntitlementDenied => {
                Self(StatusCode::FORBIDDEN, "panel_access_denied")
            }
            PanelError::RateLimited => Self(StatusCode::TOO_MANY_REQUESTS, "panel_rate_limited"),
            PanelError::UnknownTenant => Self(StatusCode::NOT_FOUND, "unknown_tenant"),
            _ => Self(StatusCode::BAD_GATEWAY, "panel_unavailable"),
        }
    }
}

pub(crate) async fn actor(state: &ApiState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    Ok(state.store.authenticate(token).await?)
}

async fn tenants(State(state): State<ApiState>) -> Result<Json<serde_json::Value>, ApiError> {
    let mut entries = state.store.public_tenants().await?;
    entries.retain(|entry| {
        entry["id"]
            .as_str()
            .and_then(|id| TenantId::parse(id).ok())
            .is_some_and(|id| state.adapters.get(&id).is_ok())
    });
    Ok(Json(serde_json::json!({"data":entries})))
}

#[derive(Deserialize)]
struct LoginInput {
    tenant_id: String,
    email: String,
    password: String,
    device: DeviceInput,
}
async fn login(
    State(state): State<ApiState>,
    Json(input): Json<LoginInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.email.len() > 254
        || input.password.len() > 1024
        || input.email.is_empty()
        || input.password.is_empty()
    {
        return Err(StoreError::InvalidInput.into());
    }
    let tenant = TenantId::parse(&input.tenant_id).map_err(|_| StoreError::InvalidInput)?;
    let provider = state.adapters.get(&tenant)?;
    state
        .store
        .check_login_license(&tenant, &input.device.platform)
        .await?;
    let session = provider
        .authenticate(&Credentials {
            email: input.email,
            password: Secret::new(input.password),
        })
        .await?;
    let user = provider.get_user(&session).await?;
    let rights = provider.get_entitlement(&session).await?;
    if !rights.usable_at(
        chrono::Utc::now()
            .timestamp()
            .try_into()
            .map_err(|_| StoreError::Unavailable)?,
    ) {
        return Err(PanelError::EntitlementDenied.into());
    }
    Ok(Json(
        serde_json::json!({"data":state.store.login(&session,&user,&input.device,rights.device_limit).await?}),
    ))
}
#[derive(Deserialize)]
struct RefreshInput {
    refresh_token: String,
}
async fn refresh(
    State(state): State<ApiState>,
    Json(input): Json<RefreshInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(
        serde_json::json!({"data":state.store.refresh(&input.refresh_token).await?}),
    ))
}
async fn logout(State(state): State<ApiState>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    state.store.logout(&actor).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn devices(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        serde_json::json!({"data":state.store.devices(&actor).await?}),
    ))
}
async fn revoke(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    state.store.revoke_device(&actor, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn register_device(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(device): Json<DeviceInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    state
        .store
        .check_login_license(&actor.tenant_id, &device.platform)
        .await?;
    let session = state.store.panel_session(&actor).await?;
    let provider = state.adapters.get(&actor.tenant_id)?;
    let user = provider.get_user(&session).await?;
    let rights = provider.get_entitlement(&session).await?;
    if !rights.usable_at(
        chrono::Utc::now()
            .timestamp()
            .try_into()
            .map_err(|_| StoreError::Unavailable)?,
    ) {
        return Err(PanelError::EntitlementDenied.into());
    }
    Ok(Json(
        serde_json::json!({"data":state.store.login(&session,&user,&device,rights.device_limit).await?}),
    ))
}
async fn me(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    if actor.tenant_id.as_str() != id {
        return Err(StoreError::Forbidden.into());
    }
    let provider = state.adapters.get(&actor.tenant_id)?;
    let user = provider
        .get_user(&state.store.panel_session(&actor).await?)
        .await?;
    Ok(Json(
        serde_json::json!({"data":{"id":actor.account_id,"email":user.email,"disabled":user.disabled,"tenant_id":id}}),
    ))
}
async fn entitlement(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    if actor.tenant_id.as_str() != id {
        return Err(StoreError::Forbidden.into());
    }
    let provider = state.adapters.get(&actor.tenant_id)?;
    let e = provider
        .get_entitlement(&state.store.panel_session(&actor).await?)
        .await?;
    Ok(Json(
        serde_json::json!({"data":{"expires_at":e.expires_at,"quota_bytes":e.quota_bytes,"upload_bytes":e.upload_bytes,"download_bytes":e.download_bytes,"device_limit":e.device_limit}}),
    ))
}

async fn app_policy(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        serde_json::json!({"data":state.store.app_policy(&actor).await?}),
    ))
}
async fn notices(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        serde_json::json!({"data":state.store.notices(&actor).await?}),
    ))
}
