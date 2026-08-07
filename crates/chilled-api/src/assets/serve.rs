//! The /ui handlers: embedded lookup, dev-dir override, SPA fallback.

use std::path::PathBuf;

use axum::extract::{Path as UrlPath, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use include_dir::{include_dir, Dir};

use crate::state::UiState;

static UI_DIST: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../dist");

/// The /ui routes. Unauthenticated by design: the shell must load so the
/// login page can render; everything sensitive is behind /api.
pub(crate) fn router() -> Router<UiState> {
    Router::new()
        .route("/ui", get(|| async { Redirect::temporary("/ui/") }))
        .route("/ui/", get(serve_index))
        .route("/ui/{*path}", get(serve_path))
}

async fn serve_index(State(state): State<UiState>) -> Response {
    serve(&state, "index.html").await
}

async fn serve_path(
    State(state): State<UiState>,
    UrlPath(path): UrlPath<String>,
    uri: Uri,
) -> Response {
    let _ = uri;
    serve(&state, &path).await
}

async fn serve(state: &UiState, path: &str) -> Response {
    if let Some(dir) = &state.config.dev_dist_dir {
        return serve_from_disk(dir.clone(), path).await;
    }
    match lookup(path) {
        Some((file_path, contents)) => file_response(&file_path, contents),
        None if UI_DIST.entries().is_empty() => (
            StatusCode::SERVICE_UNAVAILABLE,
            "this build embeds no UI and --ui-dev-dist-dir is unset; \
             build the frontend (just ui-build) or point at a dist directory",
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Embedded lookup with the SPA fallback: an extensionless miss is a client
/// route (e.g. /ui/mount/npm) and gets index.html.
fn lookup(path: &str) -> Option<(String, &'static [u8])> {
    if let Some(file) = UI_DIST.get_file(path) {
        return Some((path.to_owned(), file.contents()));
    }
    if !has_extension(path) {
        if let Some(index) = UI_DIST.get_file("index.html") {
            return Some(("index.html".to_owned(), index.contents()));
        }
    }
    None
}

/// Dev override: read from disk, refusing traversal out of the dist dir.
async fn serve_from_disk(dir: PathBuf, path: &str) -> Response {
    if path.split('/').any(|seg| seg == "..") || path.starts_with('/') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let candidate = if path.is_empty() { "index.html" } else { path };
    let full = dir.join(candidate);
    match tokio::fs::read(&full).await {
        Ok(contents) => file_response(candidate, &contents).into_response(),
        Err(_) if !has_extension(candidate) => {
            match tokio::fs::read(dir.join("index.html")).await {
                Ok(contents) => file_response("index.html", &contents),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Whether the last path segment ends in a known bundle extension. A plain
/// dot test would deny the SPA fallback to dotted mount routes (/ui/mount/corp.io).
pub(super) fn has_extension(path: &str) -> bool {
    const ASSET_EXTS: &[&str] = &[
        "html", "js", "mjs", "css", "wasm", "json", "svg", "png", "ico", "woff2", "txt", "map",
    ];
    path.rsplit('/')
        .next()
        .and_then(|seg| seg.rsplit('.').next())
        .is_some_and(|ext| ASSET_EXTS.contains(&ext))
}

pub(super) fn file_response(path: &str, contents: &[u8]) -> Response {
    // Entry files keep fixed names across releases and must revalidate; only
    // wasm-bindgen's content-hashed snippets/ files may cache immutably.
    let cache = if path.starts_with("snippets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime_for(path)),
            (header::CACHE_CONTROL, cache),
        ],
        contents.to_vec(),
    )
        .into_response()
}

pub(super) fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        // Correct type required for WebAssembly streaming instantiation.
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}
