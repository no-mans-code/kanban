use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::{EmbeddedFile, RustEmbed};

use crate::error::AppError;

/// The built frontend. Release builds embed it into the binary; debug builds
/// read it from disk, so a frontend rebuild needs no server restart.
#[derive(RustEmbed)]
#[folder = "../web/dist"]
#[allow_missing = true]
struct Assets;

fn serve(path: &str, file: EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Vite fingerprints everything under assets/, so it can be cached forever.
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    (
        [(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())],
        file.data,
    )
        .into_response()
}

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return AppError::not_found("API route").into_response();
    }
    if !path.is_empty()
        && let Some(file) = Assets::get(path)
    {
        return serve(path, file);
    }
    // Client-side routes all resolve to the app shell.
    match Assets::get("index.html") {
        Some(file) => serve("index.html", file),
        None => (StatusCode::NOT_FOUND, "The frontend is not built yet. Run `npm run build` in kanban/web.")
            .into_response(),
    }
}
