use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::ErrorBody;

pub const MAX_CLIENT_IPS: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(120);

/// Admits at most `max` distinct client IPs per sliding window. An IP stays admitted
/// while it keeps making requests and frees its slot after `window` of inactivity.
pub struct IpLimiter {
    max: usize,
    window: Duration,
    last_seen: Mutex<HashMap<IpAddr, Instant>>,
}

impl IpLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            last_seen: Mutex::default(),
        }
    }

    pub fn allow(&self, ip: IpAddr, now: Instant) -> bool {
        let ip = ip.to_canonical();
        let mut last_seen = self.last_seen.lock().unwrap();
        last_seen.retain(|_, seen| now.saturating_duration_since(*seen) < self.window);
        if last_seen.contains_key(&ip) || last_seen.len() < self.max {
            last_seen.insert(ip, now);
            true
        } else {
            false
        }
    }
}

pub async fn enforce(
    State(limiter): State<Arc<IpLimiter>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if limiter.allow(peer.ip(), Instant::now()) {
        return next.run(request).await;
    }
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, limiter.window.as_secs().to_string())],
        Json(ErrorBody {
            error: format!(
                "Only {} different IP addresses are accepted per {} seconds.",
                limiter.max,
                limiter.window.as_secs()
            ),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_three_ips_per_window_and_frees_slots_after_inactivity() {
        let limiter = IpLimiter::new(3, Duration::from_secs(120));
        let ip = |n: u8| IpAddr::from([10, 0, 0, n]);
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        assert!(limiter.allow(ip(1), at(0)));
        assert!(limiter.allow(ip(2), at(10)));
        assert!(limiter.allow(ip(3), at(20)));
        assert!(!limiter.allow(ip(4), at(30)));
        assert!(limiter.allow(ip(1), at(100)), "known IP stays admitted");
        assert!(
            !limiter.allow(ip(4), at(125)),
            "ips 1, 2 and 3 are still active"
        );
        assert!(limiter.allow(ip(4), at(135)), "ip 2 expired after 120s");
        assert!(
            !limiter.allow(ip(2), at(136)),
            "ips 1, 3 and 4 fill the window"
        );
    }
}
