//! Response building for the simple index: content negotiation, validators,
//! and the filter+rewrite (memoized) body production.

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{filtered_etag, format_etag, rewrite_etag};
use chilled_core::http::{error_response, text_response};
use log::error;
use serde_json::Value;

use crate::accept::{negotiate, Format};
use crate::constants::TEXT_CTYPE;
use crate::model::PypiEntry;
use crate::state::AppState;
use crate::{filter, render, Config};

/// A syntactically valid simple index with no files, used when a cooldown
/// cannot be honored for any of them.
pub(super) fn empty_index(name: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "meta": {"api-version": "1.0"},
        "name": name,
        "versions": [],
        "files": [],
    }))
    .unwrap_or_default()
}

fn with_vary(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

/// Serves the minimal empty project list in the negotiated format.
pub(super) fn project_list(headers: &HeaderMap) -> Response {
    let fmt = negotiate(accept_header(headers));
    let body = match fmt {
        Format::Json => r#"{"meta":{"api-version":"1.0"},"projects":[]}"#.to_owned(),
        Format::Html => "<!DOCTYPE html><html><head>\
             <meta name=\"pypi:repository-version\" content=\"1.0\">\
             <title>Simple index</title></head><body></body></html>"
            .to_owned(),
    };
    with_vary(text_response(200, fmt.ctype(), body))
}

/// 301 to the canonical project URL, built from the external mount path.
pub(super) fn redirect_to_project(config: &Config, name: &str) -> Response {
    let mut base = config.settings.proxy_url.path().to_owned();
    if !base.ends_with('/') {
        base.push('/');
    }
    let location = format!("{base}simple/{name}/");
    Response::builder()
        .status(301)
        .header(header::LOCATION, location)
        .body(Body::empty())
        .expect("valid redirect response")
}

/// The client `Accept` header value, if any.
pub(super) fn accept_header(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::ACCEPT).and_then(|v| v.to_str().ok())
}

/// The marked, format-tagged client-facing ETag for a served body. Bodies are
/// always rewritten, so the bare upstream validator is never exposed.
fn marked_etag(upstream_etag: &str, config: &Config, name: &str, fmt: Format) -> String {
    let marker = match config.serve_marker(name) {
        Some(marker) => filtered_etag(upstream_etag, marker),
        None => rewrite_etag(upstream_etag),
    };
    format_etag(&marker, fmt.tag())
}

/// Builds a project-index `304 Not Modified` response.
pub(super) fn project_not_modified(
    entry: &PypiEntry,
    config: &Config,
    name: &str,
    fmt: Format,
) -> Response {
    let mut builder = Response::builder()
        .status(304)
        .header(header::VARY, "Accept");
    if let Some(etag) = entry.meta.etag() {
        builder = builder.header(header::ETAG, marked_etag(etag, config, name, fmt));
    }
    builder.body(Body::empty()).expect("valid 304 response")
}

/// Builds the `200 OK` around an already-produced (filtered + rewritten) body.
fn project_response(
    entry: &PypiEntry,
    body: Bytes,
    config: &Config,
    name: &str,
    fmt: Format,
) -> Response {
    let mut builder = Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, fmt.ctype())
        .header(header::VARY, "Accept");
    if let Some(etag) = entry.meta.etag() {
        builder = builder.header(header::ETAG, marked_etag(etag, config, name, fmt));
    }
    builder
        .body(Body::from(body))
        .expect("valid index response")
}

/// Serves the memoized body for `entry` in the negotiated representation
/// without touching the disk cache — `None` on a memo miss.
pub(super) fn project_memo_hit(
    entry: &PypiEntry,
    state: &AppState,
    name: &str,
    fmt: Format,
) -> Option<Response> {
    let bucket = state
        .config
        .cutoff_for(name)
        .map_or(0, |c| c / MEMO_BUCKET_SECS);
    let memo_key = format!("{name}.{}", fmt.tag());
    let body = state.memo.get(&memo_key, &entry.meta.validator(), bucket)?;
    Some(project_response(entry, body, &state.config, name, fmt))
}

/// Builds a project-index `200 OK`, filtering + rewriting (and memoizing) the
/// pristine body into the negotiated representation.
pub(super) async fn project_ok(
    entry: &PypiEntry,
    data: Vec<u8>,
    state: &AppState,
    name: &str,
    fmt: Format,
) -> Response {
    let config = &state.config;
    let cutoff = config.cutoff_for(name);
    let bucket = cutoff.map_or(0, |c| c / MEMO_BUCKET_SECS);
    let validator = entry.meta.validator();
    let memo_key = format!("{name}.{}", fmt.tag());

    let body = if let Some(cached) = state.memo.get(&memo_key, &validator, bucket) {
        cached
    } else {
        let project = name.to_owned();
        let proxy_url = config.settings.proxy_url.clone();
        let produced = tokio::task::spawn_blocking(move || -> Option<Bytes> {
            let mut doc: Value = serde_json::from_slice(&data).ok()?;
            filter::filter_simple_json(&mut doc, cutoff, &project, &proxy_url);
            match fmt {
                Format::Json => serde_json::to_vec(&doc).ok().map(Bytes::from),
                Format::Html => Some(Bytes::from(render::render_html(&doc, &project))),
            }
        })
        .await;
        match produced {
            Ok(Some(bytes)) => {
                state.memo.put(memo_key, validator, bucket, bytes.clone());
                bytes
            }
            Ok(None) => {
                error!("proxy: simple index for {name} is not valid JSON");
                return text_response(502, TEXT_CTYPE, "invalid upstream simple index\n".into());
            }
            // A panic in the filter must not become an empty 200.
            Err(err) => {
                error!("cooldown: simple filter task failed for {name}: {err}");
                return error_response(500);
            }
        }
    };

    project_response(entry, body, config, name, fmt)
}
