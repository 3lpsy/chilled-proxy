//! `GET /index/<path>` — proxied, cached, age-gated sparse-index entries.

#[cfg(test)]
mod tests;

use std::path::Path;

use axum::{
    body::Body,
    extract::{Path as UrlPath, State},
    http::{header, HeaderMap},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{etag_marker, filtered_etag, unmark_etag, Marker};
use chilled_core::http::{error_response, json_response, read_capped, FetchError};
use log::{debug, error, warn};

use crate::cache::{
    cache_fetch_index_entry, cache_store_index_entry, cache_try_find_index_entry, IndexEntry,
};
use crate::config::Config;
use crate::constants::{CRATES_API_REL, INDEX_CTYPE, MAX_INDEX_SIZE};
use crate::filter;
use crate::http::format_json_error;
use crate::state::AppState;

/// Registry configuration file endpoint path (at the sparse-index root).
const CONFIG_JSON_ENDPOINT: &str = "config.json";

/// Generates the registry `config.json`, pointing crate downloads at this
/// proxy's mount. Cargo cannot handle trailing slashes here.
fn gen_config_json_file(config: &Config) -> String {
    let dl_url = config
        .settings
        .proxy_url
        .join(CRATES_API_REL)
        .expect("invalid proxy server URL");

    let dl = dl_url.as_str().trim_end_matches('/');
    let api = config.upstream_url.as_str().trim_end_matches('/');

    format!(r#"{{"dl":"{dl}","api":"{api}"}}"#)
}

/// Registry index entry download result.
pub(crate) struct IndexResponse {
    /// Index entry plus updated response metadata (etag / last-modified).
    pub(crate) entry: IndexEntry,
    /// Upstream HTTP response status code.
    pub(crate) status: u16,
    /// Upstream HTTP response body.
    pub(crate) data: Vec<u8>,
}

/// The source-content validator used as a memo key and for the weak ETag.
fn entry_validator(entry: &IndexEntry) -> String {
    entry
        .etag()
        .map(ToOwned::to_owned)
        .or_else(|| entry.last_modified())
        .unwrap_or_default()
}

/// Attaches cooldown-aware cache validators: a weak marked ETag (and no
/// `Last-Modified`) for filtered entries, the upstream validators otherwise.
fn with_index_validators(
    mut builder: axum::http::response::Builder,
    entry: &IndexEntry,
    marker: Option<Marker>,
) -> axum::http::response::Builder {
    match marker {
        Some(marker) => {
            if let Some(etag) = entry.etag() {
                builder = builder.header(header::ETAG, filtered_etag(etag, marker));
            }
        }
        None => {
            if let Some(etag) = entry.etag() {
                builder = builder.header(header::ETAG, etag);
            }
            if let Some(last_modified) = entry.last_modified() {
                builder = builder.header(header::LAST_MODIFIED, last_modified);
            }
        }
    }
    builder
}

/// Builds an index `304 Not Modified` response (no body).
fn index_not_modified(entry: &IndexEntry, config: &Config, name: &str) -> Response {
    with_index_validators(
        Response::builder().status(304),
        entry,
        config.serve_marker(name),
    )
    .body(Body::empty())
    .expect("valid 304 response")
}

/// Builds an index `200 OK` response, age-gating (and memoizing) the body when
/// the crate is subject to cooldown.
async fn index_ok(entry: &IndexEntry, data: Vec<u8>, state: &AppState, name: &str) -> Response {
    let config = &state.config;

    let Some(cutoff) = config.cutoff_for(name) else {
        // Unfiltered: serve verbatim with the upstream validators.
        return with_index_validators(
            Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, INDEX_CTYPE),
            entry,
            None,
        )
        .body(Body::from(data))
        .expect("valid index response");
    };

    let bucket = cutoff / MEMO_BUCKET_SECS;
    let validator = entry_validator(entry);

    let body = if let Some(cached) = state.memo.get(name, &validator, bucket) {
        cached
    } else {
        // Filter off the async workers; entries can be large.
        let filtered =
            match tokio::task::spawn_blocking(move || filter::filter_index(&data, cutoff)).await {
                Ok(filtered) => Bytes::from(filtered),
                // A panic in the filter must not become an empty 200.
                Err(err) => {
                    error!("cooldown: index filter task failed for {name}: {err}");
                    return error_response(500);
                }
            };
        state
            .memo
            .put(name.to_owned(), validator, bucket, filtered.clone());
        filtered
    };

    with_index_validators(
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, INDEX_CTYPE),
        entry,
        Some(Marker {
            window: config.settings.cooldown.as_secs(),
            bucket,
        }),
    )
    .body(Body::from(body))
    .expect("valid index response")
}

/// Reads a cached index entry file off the blocking thread pool.
async fn cache_read_index(dir: &Path, entry: &IndexEntry) -> Option<Vec<u8>> {
    let dir = dir.to_path_buf();
    let entry = entry.clone();
    tokio::task::spawn_blocking(move || cache_fetch_index_entry(&dir, &entry))
        .await
        .ok()
        .flatten()
}

/// Stores an index entry file off the blocking thread pool.
pub(crate) async fn cache_write_index(dir: &Path, entry: &IndexEntry, data: &[u8]) {
    let dir = dir.to_path_buf();
    let entry = entry.clone();
    let data = data.to_vec();
    let _ = tokio::task::spawn_blocking(move || cache_store_index_entry(&dir, &entry, &data)).await;
}

/// Recreates index metadata from a cache file's mtime off the blocking pool.
async fn cache_find_index(dir: &Path, name: &str) -> Option<IndexEntry> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_try_find_index_entry(&dir, &name))
        .await
        .ok()
        .flatten()
}

/// Downloads a sparse index entry from the upstream registry, sending the
/// conditional-request headers carried by `entry`.
pub(crate) async fn download_index_entry(
    state: &AppState,
    mut entry: IndexEntry,
) -> Result<IndexResponse, FetchError> {
    let url = state.config.index_url.join(&entry.to_index_url()).unwrap();

    // Pin identity encoding: a compressed body would fail the UTF-8 check and
    // pass through unfiltered, silently disabling age-gating.
    let mut request = state
        .client
        .get(url)
        .header(header::ACCEPT_ENCODING, "identity");
    if let Some(etag) = entry.etag() {
        request = request.header(header::IF_NONE_MATCH, etag);
    } else if let Some(last_modified) = entry.last_modified() {
        request = request.header(header::IF_MODIFIED_SINCE, last_modified);
    }

    let mut response = request.send().await.map_err(FetchError::Http)?;
    let status = response.status().as_u16();

    if let Some(etag) = response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
    {
        entry.set_etag(etag);
    }
    if let Some(last_modified) = response
        .headers()
        .get(header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
    {
        entry.set_last_modified(last_modified);
    }
    entry.set_last_updated();

    let data = read_capped(&mut response, MAX_INDEX_SIZE).await?;

    Ok(IndexResponse {
        entry,
        status,
        data,
    })
}

/// Fetches an index entry from upstream (or stale cache) and serves it.
///
/// `window_ok` indicates the client's cached copy was filtered at the same
/// cooldown window we serve at now; only then is a `304` safe.
async fn forward_index(
    state: &AppState,
    entry: IndexEntry,
    cached_entry: Option<IndexEntry>,
    name: &str,
    window_ok: bool,
) -> Response {
    // Revalidate only against our own cached metadata. Forwarding the client's
    // validator with nothing cached would turn an upstream 304 into a 503 we
    // could never escape.
    let req_entry = cached_entry.unwrap_or_else(|| IndexEntry::new(name));

    let response = match download_index_entry(state, req_entry).await {
        Ok(response) => response,
        Err(err) => {
            // Transport failure: serve a possibly-stale cached copy if present.
            if let Some(data) = cache_read_index(&state.config.index_dir, &entry).await {
                warn!("proxy: forwarding possibly stale cached index for {name}: {err}");
                // Label the stale body with validators derived from the cache
                // file, never the client's — they describe a different body.
                let stale = cache_find_index(&state.config.index_dir, name)
                    .await
                    .unwrap_or_else(|| IndexEntry::new(name));
                return index_ok(&stale, data, state, name).await;
            }
            error!("fetch: index connection failed for {name}: {err}");
            return json_response(502, format_json_error(err));
        }
    };

    match response.status {
        200 => {
            debug!("fetch: successfully got index entry for {name}");
            cache_write_index(&state.config.index_dir, &response.entry, &response.data).await;
            state.metadata.store(name, response.entry.clone());

            if window_ok && response.entry.is_equivalent(&entry) {
                index_not_modified(&response.entry, &state.config, name)
            } else {
                index_ok(&response.entry, response.data, state, name).await
            }
        }
        304 => {
            debug!("fetch: cached index entry for {name} is up to date");
            state.metadata.store(name, response.entry.clone());

            if window_ok && response.entry.is_equivalent(&entry) {
                index_not_modified(&response.entry, &state.config, name)
            } else if let Some(data) = cache_read_index(&state.config.index_dir, &entry).await {
                index_ok(&response.entry, data, state, name).await
            } else {
                error!("cache: lost index cache file for {name}");
                state.metadata.invalidate(name);
                error_response(503)
            }
        }
        code => {
            // Forward other upstream statuses (e.g. 404) verbatim.
            warn!("fetch: upstream returned HTTP status {code} for {name}");
            json_response(code, String::from_utf8_lossy(&response.data).into_owned())
        }
    }
}

/// Handles a sparse registry index request: `GET /index/<path>`.
pub(crate) async fn handle_index(
    State(state): State<AppState>,
    UrlPath(path): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    if path == CONFIG_JSON_ENDPOINT {
        debug!("proxy: sending registry config file");
        return json_response(200, gen_config_json_file(&state.config));
    }

    let Some(mut index_entry) = IndexEntry::try_from_index_url(&path) else {
        warn!("proxy: malformed registry index path: {path}");
        return error_response(404);
    };
    let name = index_entry.name().to_owned();

    // Undo our cooldown ETag marker so the upstream conditional GET uses the
    // real validator; remember the marker's window — a 304 is only safe if it
    // matches the window we serve at now.
    let mut client_marker = None;
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        index_entry.set_etag(&unmark_etag(inm));
        client_marker = etag_marker(inm);
    } else if let Some(ims) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
    {
        index_entry.set_last_modified(ims);
    }

    let window_ok = client_marker == state.config.serve_marker(&name);

    // Serve from cache when the metadata cache is warm and unexpired.
    if let Some(cached_entry) = state.metadata.fetch(&name) {
        if cached_entry.is_expired_with_ttl(&state.config.settings.cache_ttl) {
            debug!("proxy: index cache expired for {name}, refreshing...");
            return forward_index(&state, index_entry, Some(cached_entry), &name, window_ok).await;
        }

        if window_ok && cached_entry.is_equivalent(&index_entry) {
            debug!("proxy: index metadata cache hit for {name}");
            return index_not_modified(&cached_entry, &state.config, &name);
        }

        if let Some(data) = cache_read_index(&state.config.index_dir, &index_entry).await {
            debug!("proxy: index data cache hit for {name}");
            return index_ok(&cached_entry, data, &state, &name).await;
        }
    }

    // Recreate metadata from the cache file mtime, then fetch from upstream.
    let mtimed_entry = cache_find_index(&state.config.index_dir, &name).await;
    forward_index(&state, index_entry, mtimed_entry, &name, window_ok).await
}
