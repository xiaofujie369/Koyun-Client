use crate::{
    api::{ApiError, ApiState, actor},
    managed::ProfileState,
    panel::PanelError,
    store::{Principal, StoreError},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use uuid::Uuid;

async fn synchronize(state: &ApiState, actor: &Principal) -> Result<ProfileState, ApiError> {
    let profile = state.store.managed_state(actor).await?;
    let started = Utc::now();
    if profile.status == "active"
        && profile
            .last_sync_at
            .is_some_and(|last| started - last < Duration::seconds(30))
    {
        return Ok(profile);
    }
    let provider = state.adapters.get(&actor.tenant_id)?;
    let session = state.store.panel_session(actor).await?;
    let fetched = async {
        let user = provider.get_user(&session).await?;
        if user.disabled {
            return Err(PanelError::AccountDisabled);
        }
        let rights = provider.get_entitlement(&session).await?;
        if !rights.usable_at(
            started
                .timestamp()
                .try_into()
                .map_err(|_| PanelError::Unavailable)?,
        ) {
            return Err(PanelError::EntitlementDenied);
        }
        let document = provider.get_subscription(&session).await?;
        Ok((rights, document))
    }
    .await;
    match fetched {
        Ok((rights, document)) => {
            state.store.save_entitlement(actor, &rights).await?;
            Ok(state.store.cache_profile(actor, &document, started).await?)
        }
        Err(error) => {
            if matches!(
                error,
                PanelError::InvalidCredentials
                    | PanelError::SessionExpired
                    | PanelError::AccountDisabled
                    | PanelError::EntitlementDenied
            ) {
                state.store.suspend_profile(actor).await?;
            }
            Err(error.into())
        }
    }
}

pub(crate) async fn profiles(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        serde_json::json!({"data":[synchronize(&state,&actor).await?]}),
    ))
}
pub(crate) async fn profile_state(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    if state.store.managed_state(&actor).await?.id != id {
        return Err(StoreError::NotFound.into());
    }
    Ok(Json(
        serde_json::json!({"data":synchronize(&state,&actor).await?}),
    ))
}
pub(crate) async fn content(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let actor = actor(&state, &headers).await?;
    if state.store.managed_state(&actor).await?.id != id {
        return Err(StoreError::NotFound.into());
    }
    synchronize(&state, &actor).await?;
    let (profile, document) = state.store.managed_content(&actor, id).await?;
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|v| v.trim() == document.etag() || v.trim() == "*")
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        document.content().to_vec().into_response()
    };
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/yaml"),
    );
    response.headers_mut().insert(
        header::ETAG,
        document
            .etag()
            .parse()
            .map_err(|_| StoreError::Unavailable)?,
    );
    response.headers_mut().insert(
        "x-profile-version",
        profile
            .version
            .to_string()
            .parse()
            .map_err(|_| StoreError::Unavailable)?,
    );
    Ok(response)
}
pub(crate) async fn sync_state(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    synchronize(&state, &actor).await?;
    Ok(Json(
        serde_json::json!({"data":state.store.sync_versions(&actor).await?}),
    ))
}
