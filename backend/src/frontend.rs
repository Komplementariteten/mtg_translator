use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// The built frontend, embedded into release binaries (read from disk in debug builds).
#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Assets::get(path) {
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, file.metadata.mimetype().to_string()),
                (header::CACHE_CONTROL, cache.to_string()),
            ],
            file.data,
        )
            .into_response();
    }
    if path.starts_with("api/") || path.starts_with("images/") || path.contains('.') {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Assets::get("index.html") {
        Some(index) => ([(header::CONTENT_TYPE, "text/html")], index.data).into_response(),
        None => (StatusCode::NOT_FOUND, "Frontend has not been built.").into_response(),
    }
}

/// Number of embedded frontend files and whether `index.html` is among them.
pub fn embedded() -> (usize, bool) {
    (Assets::iter().count(), Assets::get("index.html").is_some())
}
