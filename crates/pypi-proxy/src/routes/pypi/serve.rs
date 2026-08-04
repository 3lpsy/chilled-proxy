//! Serving the simple index: validators, the cache/upstream ladder,
//! filtering, and content negotiation.

use std::path::Path;

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{
    etag_format, etag_marker, filtered_etag, format_etag, rewrite_etag, unmark_etag,
};
use chilled_core::http::{error_response, text_response};
use log::{debug, error, warn};
use serde_json::Value;

use crate::accept::{negotiate, Format};
use crate::constants::TEXT_CTYPE;
use crate::model::{cache_fetch_simple, cache_store_simple, cache_try_find_simple, PypiEntry};
use crate::routes::pypi::fetch::{download_simple, is_json_simple, passthrough_response};
use crate::state::AppState;
use crate::{filter, render, Config};

pub(super) fn with_vary(mut response: Response) -> Response {
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
fn accept_header(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::ACCEPT).and_then(|v| v.to_str().ok())
}

/// The source-content validator used as a memo key and for the weak ETag.
pub(super) fn entry_validator(entry: &PypiEntry) -> String {
    entry
        .etag()
        .map(ToOwned::to_owned)
        .or_else(|| entry.last_modified())
        .unwrap_or_default()
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
fn project_not_modified(entry: &PypiEntry, config: &Config, name: &str, fmt: Format) -> Response {
    let mut builder = Response::builder()
        .status(304)
        .header(header::VARY, "Accept");
    if let Some(etag) = entry.etag() {
        builder = builder.header(header::ETAG, marked_etag(etag, config, name, fmt));
    }
    builder.body(Body::empty()).expect("valid 304 response")
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
    let validator = entry_validator(entry);
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

    let mut builder = Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, fmt.ctype())
        .header(header::VARY, "Accept");
    if let Some(etag) = entry.etag() {
        builder = builder.header(header::ETAG, marked_etag(etag, config, name, fmt));
    }
    builder
        .body(Body::from(body))
        .expect("valid index response")
}

/// Reads the cached pristine simple index off the blocking thread pool.
pub(super) async fn cache_read_simple(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_fetch_simple(&dir, &name))
        .await
        .ok()
        .flatten()
}

/// Stores a pristine simple index off the blocking thread pool.
pub(super) async fn cache_write_simple(dir: &Path, entry: &PypiEntry, data: &[u8]) {
    let dir = dir.to_path_buf();
    let entry = entry.clone();
    let data = data.to_vec();
    let _ = tokio::task::spawn_blocking(move || cache_store_simple(&dir, &entry, &data)).await;
}

/// Recreates entry metadata from the cache file's mtime off the blocking pool.
pub(super) async fn cache_find_simple(dir: &Path, name: &str) -> Option<PypiEntry> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_try_find_simple(&dir, &name))
        .await
        .ok()
        .flatten()
}

/// cooldown window *and* representation we serve now; only then is a 304 safe.
async fn forward_project(
    state: &AppState,
    entry: PypiEntry,
    cached_entry: Option<PypiEntry>,
    name: &str,
    window_ok: bool,
    fmt: Format,
) -> Response {
    // Revalidate only against our own cached metadata. Forwarding the client's
    // validator with nothing cached would turn an upstream 304 into a 503 we
    // could never escape.
    let req_entry = cached_entry.unwrap_or_else(|| PypiEntry::new(name));

    let response = match download_simple(state, req_entry).await {
        Ok(response) => response,
        Err(err) => {
            // Transport failure: serve a possibly-stale cached copy if present.
            if let Some(data) = cache_read_simple(&state.config.simple_dir, name).await {
                warn!("proxy: forwarding possibly stale cached simple index for {name}: {err}");
                // Label the stale body with validators derived from the cache
                // file, never the client's — they describe a different body.
                let stale = cache_find_simple(&state.config.simple_dir, name)
                    .await
                    .unwrap_or_else(|| PypiEntry::new(name));
                return project_ok(&stale, data, state, name, fmt).await;
            }
            error!("fetch: simple index connection failed for {name}: {err}");
            return text_response(502, TEXT_CTYPE, format!("upstream fetch failed: {err}\n"));
        }
    };

    match response.status {
        200 if response.undatable && state.config.cutoff_for(name).is_some() => {
            error!(
                "cooldown: simple index for {name} carries no upload times; \
                 refusing to serve ungated"
            );
            text_response(
                502,
                TEXT_CTYPE,
                "upstream index carries no upload times to age-gate on\n".into(),
            )
        }
        200 if !is_json_simple(&response.ctype) => {
            if state.config.cutoff_for(name).is_some() {
                error!(
                    "cooldown: upstream served neither JSON nor HTML simple index for {name}; \
                     refusing to serve ungated"
                );
                text_response(
                    502,
                    TEXT_CTYPE,
                    "upstream did not provide a usable simple index\n".into(),
                )
            } else {
                warn!("proxy: passing through unrecognized simple index for {name}; file URLs are unproxied");
                passthrough_response(&response.ctype, response.data)
            }
        }
        200 => {
            debug!("fetch: successfully got simple index for {name}");
            cache_write_simple(&state.config.simple_dir, &response.entry, &response.data).await;
            state.metadata.store(name, response.entry.clone());

            if window_ok && response.entry.is_equivalent(&entry) {
                project_not_modified(&response.entry, &state.config, name, fmt)
            } else {
                project_ok(&response.entry, response.data, state, name, fmt).await
            }
        }
        304 => {
            debug!("fetch: cached simple index for {name} is up to date");
            state.metadata.store(name, response.entry.clone());

            if window_ok && response.entry.is_equivalent(&entry) {
                project_not_modified(&response.entry, &state.config, name, fmt)
            } else if let Some(data) = cache_read_simple(&state.config.simple_dir, name).await {
                project_ok(&response.entry, data, state, name, fmt).await
            } else {
                error!("cache: lost simple index cache file for {name}");
                state.metadata.invalidate(name);
                error_response(503)
            }
        }
        code => {
            // Forward other upstream statuses (e.g. 404) verbatim.
            warn!("fetch: upstream returned HTTP status {code} for {name}");
            text_response(
                code,
                TEXT_CTYPE,
                String::from_utf8_lossy(&response.data).into_owned(),
            )
        }
    }
}

/// Handles `GET /simple/{name}/` for an already-normalized project name.
pub(super) async fn serve_project(state: &AppState, name: &str, headers: &HeaderMap) -> Response {
    let fmt = negotiate(accept_header(headers));

    // Undo our ETag marker so upstream revalidation uses the real validator;
    // a 304 is only safe when the marker's window and format match ours.
    let mut entry = PypiEntry::new(name);
    let mut client_marker = None;
    let mut client_fmt = None;
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        entry.set_etag(&unmark_etag(inm));
        client_marker = etag_marker(inm);
        client_fmt = etag_format(inm);
    }
    let window_ok =
        client_marker == state.config.serve_marker(name) && client_fmt == Some(fmt.tag());

    // Serve from cache when the metadata cache is warm and unexpired.
    if let Some(cached_entry) = state.metadata.fetch(name) {
        if cached_entry.is_expired_with_ttl(&state.config.settings.cache_ttl) {
            debug!("proxy: simple cache expired for {name}, refreshing...");
            return forward_project(state, entry, Some(cached_entry), name, window_ok, fmt).await;
        }

        if window_ok && cached_entry.is_equivalent(&entry) {
            debug!("proxy: simple metadata cache hit for {name}");
            return project_not_modified(&cached_entry, &state.config, name, fmt);
        }

        if let Some(data) = cache_read_simple(&state.config.simple_dir, name).await {
            debug!("proxy: simple data cache hit for {name}");
            return project_ok(&cached_entry, data, state, name, fmt).await;
        }
    }

    // Recreate metadata from the cache file mtime, then fetch from upstream.
    let mtimed_entry = cache_find_simple(&state.config.simple_dir, name).await;
    forward_project(state, entry, mtimed_entry, name, window_ok, fmt).await
}

/// Whether this file may be downloaded under `--restrict-downloads`.
///
/// The file's `upload-time` is read from the locally cached *pristine* simple
/// index. **Fail-closed**: no cached index, unknown filename, or a too-new
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_validator_prefers_etag() {
        let mut entry = PypiEntry::new("requests");
        assert_eq!(entry_validator(&entry), "");
        entry.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(entry_validator(&entry), "Sun, 06 Nov 1994 08:49:37 GMT");
        entry.set_etag("\"abc\"");
        assert_eq!(entry_validator(&entry), "\"abc\"");
    }
}
