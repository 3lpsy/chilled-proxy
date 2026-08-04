//! Serving sparse-index entries: the generated `config.json`, HTTP
//! validators, the cooldown filter, and the on-disk cache.

use std::path::Path;

use axum::{body::Body, http::header, response::Response};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{filtered_etag, Marker};
use chilled_core::http::error_response;
use log::error;

use crate::cache::{
    cache_fetch_index_entry, cache_store_index_entry, cache_try_find_index_entry, IndexEntry,
};
use crate::config::Config;
use crate::constants::{CRATES_API_REL, INDEX_CTYPE};
use crate::filter;
use crate::state::AppState;

/// Registry configuration file endpoint path (at the sparse-index root).
pub(crate) const CONFIG_JSON_ENDPOINT: &str = "config.json";

/// Generates the registry `config.json`, pointing crate downloads at this
/// proxy's mount. Cargo cannot handle trailing slashes here.
pub(crate) fn gen_config_json_file(config: &Config) -> String {
    let dl_url = config
        .settings
        .proxy_url
        .join(CRATES_API_REL)
        .expect("invalid proxy server URL");

    let dl = dl_url.as_str().trim_end_matches('/');
    let api = config.upstream_url.as_str().trim_end_matches('/');

    format!(r#"{{"dl":"{dl}","api":"{api}"}}"#)
}

/// The source-content validator used as a memo key and for the weak ETag.
pub(crate) fn entry_validator(entry: &IndexEntry) -> String {
    entry
        .etag()
        .map(ToOwned::to_owned)
        .or_else(|| entry.last_modified())
        .unwrap_or_default()
}

/// Attaches cooldown-aware cache validators: a weak marked ETag (and no
/// `Last-Modified`) for filtered entries, the upstream validators otherwise.
pub(crate) fn with_index_validators(
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
pub(crate) fn index_not_modified(entry: &IndexEntry, config: &Config, name: &str) -> Response {
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
pub(crate) async fn index_ok(
    entry: &IndexEntry,
    data: Vec<u8>,
    state: &AppState,
    name: &str,
) -> Response {
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
pub(crate) async fn cache_read_index(dir: &Path, entry: &IndexEntry) -> Option<Vec<u8>> {
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
pub(crate) async fn cache_find_index(dir: &Path, name: &str) -> Option<IndexEntry> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_try_find_index_entry(&dir, &name))
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests;
