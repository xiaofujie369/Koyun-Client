use crate::{
    api::{ApiError, ApiState, actor},
    crypto::{TokenKind, generate_token, inspect_token},
    store::{Principal, Store, StoreError},
};
use async_trait::async_trait;
use axum::{
    Json,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::Response,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

type ConnectionKey = (String, uuid::Uuid);
static CONNECTIONS: LazyLock<Mutex<HashMap<ConnectionKey, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
struct ConnectionLease(ConnectionKey);
impl ConnectionLease {
    fn acquire(actor: &Principal) -> Result<Self, StoreError> {
        let key = (actor.tenant_id.as_str().to_owned(), actor.session_id);
        let mut entries = CONNECTIONS.lock().map_err(|_| StoreError::Unavailable)?;
        if entries.values().sum::<u32>() >= 256 || entries.get(&key).copied().unwrap_or(0) >= 2 {
            return Err(StoreError::Unavailable);
        }
        *entries.entry(key.clone()).or_default() += 1;
        Ok(Self(key))
    }
}
impl Drop for ConnectionLease {
    fn drop(&mut self) {
        if let Ok(mut entries) = CONNECTIONS.lock()
            && let Some(count) = entries.get_mut(&self.0)
        {
            *count -= 1;
            if *count == 0 {
                entries.remove(&self.0);
            }
        }
    }
}

impl Store {
    pub async fn realtime_ticket(&self, actor: &Principal) -> Result<String, StoreError> {
        self.check_realtime_session(actor).await?;
        let token = generate_token(&actor.tenant_id, TokenKind::Realtime)
            .map_err(|_| StoreError::Unavailable)?;
        let mut tx = self.begin(&actor.tenant_id).await?;
        sqlx::query("SELECT id FROM sessions WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
            .bind(actor.tenant_id.as_str())
            .bind(actor.session_id)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM realtime_tickets WHERE tenant_id=$1 AND (expires_at<now() OR consumed_at IS NOT NULL)").bind(actor.tenant_id.as_str()).execute(&mut *tx).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM realtime_tickets WHERE tenant_id=$1 AND session_id=$2",
        )
        .bind(actor.tenant_id.as_str())
        .bind(actor.session_id)
        .fetch_one(&mut *tx)
        .await?;
        if count >= 4 {
            return Err(StoreError::Forbidden);
        }
        sqlx::query("INSERT INTO realtime_tickets(tenant_id,token_hash,session_id,expires_at) VALUES($1,$2,$3,now()+interval '60 seconds')")
            .bind(actor.tenant_id.as_str()).bind(token.hash.as_slice()).bind(actor.session_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(token.plaintext.expose().into())
    }
    pub async fn consume_realtime_ticket(&self, token: &str) -> Result<Principal, StoreError> {
        let (tenant, hash) =
            inspect_token(token, TokenKind::Realtime).map_err(|_| StoreError::Unauthorized)?;
        let mut tx = self.begin(&tenant).await?;
        let row=sqlx::query("UPDATE realtime_tickets r SET consumed_at=now() FROM sessions s WHERE r.tenant_id=$1 AND r.token_hash=$2 AND r.consumed_at IS NULL AND r.expires_at>now() AND s.tenant_id=r.tenant_id AND s.id=r.session_id RETURNING s.id,s.account_id,s.device_id")
            .bind(tenant.as_str()).bind(hash.as_slice()).fetch_optional(&mut *tx).await?.ok_or(StoreError::Unauthorized)?;
        let actor = Principal {
            tenant_id: tenant,
            session_id: row.get("id"),
            account_id: row.get("account_id"),
            device_id: row.get("device_id"),
        };
        tx.commit().await?;
        self.check_realtime_session(&actor).await?;
        Ok(actor)
    }
    pub async fn check_realtime_session(&self, actor: &Principal) -> Result<(), StoreError> {
        let mut tx = self.begin(&actor.tenant_id).await?;
        let platform:String=sqlx::query_scalar("SELECT d.platform FROM sessions s JOIN devices d ON d.tenant_id=s.tenant_id AND d.id=s.device_id JOIN managed_accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id JOIN tenant_licenses l ON l.tenant_id=s.tenant_id WHERE s.tenant_id=$1 AND s.id=$2 AND s.account_id=$3 AND s.device_id=$4 AND s.revoked_at IS NULL AND s.access_expires_at>now() AND s.expires_at>now() AND d.revoked_at IS NULL AND a.status='active' AND l.allow_realtime")
            .bind(actor.tenant_id.as_str()).bind(actor.session_id).bind(actor.account_id).bind(actor.device_id).fetch_optional(&mut *tx).await?.ok_or(StoreError::Unauthorized)?;
        Self::license(&mut tx, &actor.tenant_id, &platform, Utc::now()).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
pub trait RealtimeProvider: Send + Sync {
    async fn snapshot(&self, actor: &Principal) -> Result<Value, StoreError>;
}
pub struct DatabaseRealtimeProvider(pub Arc<Store>);
#[async_trait]
impl RealtimeProvider for DatabaseRealtimeProvider {
    async fn snapshot(&self, actor: &Principal) -> Result<Value, StoreError> {
        self.0.check_realtime_session(actor).await?;
        self.0.sync_versions(actor).await
    }
}
#[derive(Default)]
pub struct RealtimeEventBus {
    previous: Option<Value>,
}
impl RealtimeEventBus {
    pub fn publish(&mut self, state: Value) -> Vec<Value> {
        let mut events = Vec::new();
        if let Some(previous) = &self.previous {
            for (field, event) in [
                ("profile_version", "profile.changed"),
                ("entitlement_version", "entitlement.changed"),
                ("policy_version", "tenant.policy.changed"),
            ] {
                if previous[field] != state[field] {
                    events.push(
                        json!({"type":event,"tenant_id":state["tenant_id"],"version":state[field]}),
                    );
                }
            }
        } else {
            events.push(json!({"type":"sync.required","tenant_id":state["tenant_id"]}));
        }
        self.previous = Some(state);
        events
    }
}
pub(crate) async fn ticket(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(
        json!({"data":{"token":state.store.realtime_ticket(&actor).await?,"expires_in":60}}),
    ))
}
pub(crate) async fn upgrade(
    State(state): State<ApiState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let ticket = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StoreError::Unauthorized)?;
    let actor = state.store.consume_realtime_ticket(ticket).await?;
    let lease = ConnectionLease::acquire(&actor)?;
    Ok(ws
        .max_message_size(1024)
        .max_frame_size(1024)
        .on_upgrade(move |socket| async move {
            let _lease = lease;
            serve(socket, actor, DatabaseRealtimeProvider(state.store)).await
        }))
}
async fn serve(mut socket: WebSocket, actor: Principal, provider: impl RealtimeProvider) {
    let mut bus = RealtimeEventBus::default();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _=tick.tick()=>{
                let snapshot=match tokio::time::timeout(std::time::Duration::from_secs(5),provider.snapshot(&actor)).await {Ok(Ok(value))=>value,_=>break};
                let mut messages=bus.publish(snapshot).into_iter().map(|event|Message::Text(event.to_string().into())).collect::<Vec<_>>();
                messages.push(Message::Ping(Vec::new().into()));
                for message in messages{if !matches!(tokio::time::timeout(std::time::Duration::from_secs(5),socket.send(message)).await,Ok(Ok(()))){return;}}
            }
            incoming=socket.recv()=>{if matches!(incoming,None|Some(Err(_))|Some(Ok(Message::Close(_)))){break;}}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_sync_then_only_changed_versions() {
        let mut bus = RealtimeEventBus::default();
        let mut state = json!({"tenant_id":"demo","profile_version":1,"entitlement_version":1,"policy_version":1});
        assert_eq!(bus.publish(state.clone())[0]["type"], "sync.required");
        assert!(bus.publish(state.clone()).is_empty());
        state["profile_version"] = json!(2);
        let events = bus.publish(state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "profile.changed");
        assert!(events[0].get("content").is_none());
    }
}
