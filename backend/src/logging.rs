use std::{net::SocketAddr, time::Instant};

use axum::{
    extract::{ConnectInfo, Request},
    http::header,
    middleware::Next,
    response::Response,
};

/// Logs every request with client IP, status, duration and (truncated) User-Agent.
pub async fn requests(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let target = request
        .uri()
        .path_and_query()
        .map_or_else(|| request.uri().path().to_string(), ToString::to_string);
    let agent: String = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .chars()
        .take(60)
        .collect();
    let start = Instant::now();
    let response = next.run(request).await;
    let status = response.status();
    let line = format!(
        "[request] {} {method} {target} -> {} in {} ms | {agent}",
        peer.ip(),
        status.as_u16(),
        start.elapsed().as_millis()
    );
    if status.is_server_error() {
        eprintln!("{line} (server error)");
    } else if status.as_u16() == 429 {
        eprintln!("{line} (rejected: IP limit)");
    } else {
        println!("{line}");
    }
    response
}
