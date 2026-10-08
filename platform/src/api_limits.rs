use axum::{
    extract::{ConnectInfo, Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Limits(Mutex<HashMap<IpAddr, (Instant, u32)>>);
impl Limits {
    fn allow(&self, ip: IpAddr, now: Instant) -> bool {
        let Ok(mut entries) = self.0.lock() else {
            return false;
        };
        entries.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(60));
        if !entries.contains_key(&ip) && entries.len() >= 4096 {
            return false;
        }
        let (_, count) = entries.entry(ip).or_insert((now, 0));
        if *count >= 20 {
            return false;
        }
        *count += 1;
        true
    }
}
pub(crate) async fn guard(
    State(limits): State<Arc<Limits>>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(request.uri().path(), "/v1/auth/login" | "/v1/auth/refresh") {
        let ip = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|peer| peer.0.ip());
        if !ip.is_some_and(|ip| limits.allow(ip, Instant::now())) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, "60")],
                axum::Json(serde_json::json!({"error":{"code":"rate_limited"}})),
            )
                .into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_are_per_peer_bounded_and_expire() {
        let limits = Limits::default();
        let now = Instant::now();
        let ip = "127.0.0.1".parse().unwrap();
        for _ in 0..20 {
            assert!(limits.allow(ip, now));
        }
        assert!(!limits.allow(ip, now));
        assert!(limits.allow("127.0.0.2".parse().unwrap(), now));
        assert!(limits.allow(ip, now + Duration::from_secs(60)));
        for value in 1..=4096u32 {
            limits.allow(IpAddr::V4(value.into()), now + Duration::from_secs(60));
        }
        assert!(!limits.allow("192.0.2.1".parse().unwrap(), now + Duration::from_secs(60)));
        assert!(limits.0.lock().unwrap().len() <= 4096);
    }
}
