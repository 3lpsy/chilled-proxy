//! The metadata serve ladder (mirrors the crates-proxy index route):
//! validators, warm metadata, disk cache, then upstream.

use std::path::PathBuf;

use axum::{body::Body, http::header, http::HeaderMap, response::Response};
use bytes::Bytes;
use chilled_core::cache::fs::{fetch_file_async, file_mtime_async, store_file_async};
use chilled_core::etag::{cooldown_validators, etag_marker, unmark_etag};
use chilled_core::http::{conditional_get, text_response, FetchError};
use log::{debug, error, warn};

use crate::checksum::ChecksumAlgo;
use crate::constants::TEXT_CTYPE;
use crate::coords::MavenCoords;
use crate::model::MavenEntry;
use crate::routes::maven::handler::plain_error;
use crate::routes::maven::metadata::output::{metadata_memo_hit, metadata_ok};
use crate::routes::maven::metadata::passthrough::pass_through;
use crate::sidecar::SIDECAR_FILE;
use crate::state::AppState;

/// Metadata download result from upstream.
struct MetaResponse {
    entry: MavenEntry,
    status: u16,
    data: Vec<u8>,
}

/// Absolute cache path of the pristine metadata file.
fn metadata_cache_path(state: &AppState, coords: &MavenCoords) -> PathBuf {
    state.config.repo_dir.join(coords.metadata_rel())
}

/// Absolute cache path of the artifact's version-age sidecar.
pub(crate) fn sidecar_path(state: &AppState, coords: &MavenCoords) -> PathBuf {
    state
        .config
        .repo_dir
        .join(coords.dir_rel())
        .join(SIDECAR_FILE)
}

/// Builds a metadata `304 Not Modified` response (no body).
fn metadata_not_modified(entry: &MavenEntry, state: &AppState, coords: &MavenCoords) -> Response {
    cooldown_validators(
        Response::builder().status(304),
        entry,
        state.config.serve_marker(coords),
    )
    .body(Body::empty())
    .expect("valid 304 response")
}

/// Reads the pristine cached metadata file off the blocking thread pool.
async fn cache_read_metadata(state: &AppState, coords: &MavenCoords) -> Option<Vec<u8>> {
    fetch_file_async(metadata_cache_path(state, coords)).await
}

/// Recreates metadata validators from the cache file's mtime.
async fn cache_find_metadata(state: &AppState, coords: &MavenCoords) -> Option<MavenEntry> {
    let mtime = file_mtime_async(metadata_cache_path(state, coords)).await?;
    let mut entry = MavenEntry::new();
    entry.set_mtime(mtime);
    Some(entry)
}

/// Downloads `maven-metadata.xml` from upstream with conditional headers.
async fn download_metadata(
    state: &AppState,
    coords: &MavenCoords,
    mut entry: MavenEntry,
) -> Result<MetaResponse, FetchError> {
    let url = state
        .config
        .upstream_url
        .join(&coords.metadata_rel())
        .expect("validated segments join onto the pinned upstream URL");

    let response = conditional_get(
        &state.client,
        url,
        None,
        &mut entry,
        state.config.settings.max_metadata_size,
    )
    .await?;

    Ok(MetaResponse {
        entry,
        status: response.status,
        data: response.data,
    })
}

/// Serves artifact-level metadata (or a generated checksum of it).
pub(crate) async fn serve_metadata(
    state: &AppState,
    coords: &MavenCoords,
    algo: Option<ChecksumAlgo>,
    headers: &HeaderMap,
) -> Response {
    // No cooldown: metadata checksums are upstream's own files, verbatim.
    if let Some(algo) = algo {
        if state.config.cutoff_for(coords).is_none() {
            let rel = format!("{}.{}", coords.metadata_rel(), algo.ext());
            return pass_through(state, &rel, TEXT_CTYPE).await;
        }
    }

    let key = coords.dir_rel();
    let mut client_entry = MavenEntry::new();
    let mut client_marker = None;

    // Conditional requests only apply to the `.xml` body itself.
    if algo.is_none() {
        if let Some(inm) = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
        {
            client_entry.set_etag(&unmark_etag(inm));
            client_marker = etag_marker(inm);
        } else if let Some(ims) = headers
            .get(header::IF_MODIFIED_SINCE)
            .and_then(|v| v.to_str().ok())
        {
            client_entry.set_last_modified(ims);
        }
    }
    let window_ok = algo.is_none() && client_marker == state.config.serve_marker(coords);

    // Serve from cache when the metadata cache is warm and unexpired.
    if let Some(cached_entry) = state.metadata.fetch(&key) {
        if cached_entry.is_expired_with_ttl(&state.config.settings.cache_ttl) {
            debug!("proxy: metadata cache expired for {coords}, refreshing...");
            return forward_metadata(
                state,
                coords,
                client_entry,
                Some(cached_entry),
                algo,
                window_ok,
            )
            .await;
        }
        if window_ok && cached_entry.is_equivalent(&client_entry) {
            debug!("proxy: metadata cache hit for {coords}");
            return metadata_not_modified(&cached_entry, state, coords);
        }
        // A memo hit needs no pristine body, so skip the disk read entirely.
        if let Some(response) = metadata_memo_hit(state, coords, &cached_entry, algo).await {
            debug!("proxy: metadata memo hit for {coords}");
            return response;
        }
        if let Some(data) = cache_read_metadata(state, coords).await {
            debug!("proxy: metadata data cache hit for {coords}");
            return metadata_ok(state, coords, &cached_entry, data, algo).await;
        }
    }

    // Recreate validators from the cache file mtime, then fetch from upstream.
    let mtimed_entry = cache_find_metadata(state, coords).await;
    forward_metadata(state, coords, client_entry, mtimed_entry, algo, window_ok).await
}

/// Fetches metadata from upstream (or stale cache) and serves it.
async fn forward_metadata(
    state: &AppState,
    coords: &MavenCoords,
    client_entry: MavenEntry,
    cached_entry: Option<MavenEntry>,
    algo: Option<ChecksumAlgo>,
    window_ok: bool,
) -> Response {
    // Revalidate only against our own cached metadata. Forwarding the client's
    // validator with nothing cached would turn an upstream 304 into a 503 we
    // could never escape.
    let req_entry = cached_entry.clone().unwrap_or_default();

    let response = match download_metadata(state, coords, req_entry).await {
        Ok(response) => response,
        Err(err) => {
            // Transport failure: serve possibly-stale pristine cache (still filtered).
            if let Some(data) = cache_read_metadata(state, coords).await {
                warn!("proxy: forwarding possibly stale cached metadata for {coords}: {err}");
                let stale = cache_find_metadata(state, coords)
                    .await
                    .or(cached_entry)
                    .unwrap_or_else(MavenEntry::new);
                return metadata_ok(state, coords, &stale, data, algo).await;
            }
            error!("fetch: metadata connection failed for {coords}: {err}");
            return plain_error(502, "upstream fetch failed");
        }
    };

    let key = coords.dir_rel();
    match response.status {
        200 => {
            debug!("fetch: successfully got metadata for {coords}");
            store_file_async(
                metadata_cache_path(state, coords),
                Bytes::from(response.data.clone()),
                response.entry.mtime(),
            )
            .await;
            state.metadata.store(&key, response.entry.clone());

            if window_ok && response.entry.is_equivalent(&client_entry) {
                metadata_not_modified(&response.entry, state, coords)
            } else {
                metadata_ok(state, coords, &response.entry, response.data, algo).await
            }
        }
        304 => {
            debug!("fetch: cached metadata for {coords} is up to date");
            state.metadata.store(&key, response.entry.clone());

            if window_ok && response.entry.is_equivalent(&client_entry) {
                metadata_not_modified(&response.entry, state, coords)
            } else if let Some(resp) = metadata_memo_hit(state, coords, &response.entry, algo).await
            {
                // A memo hit needs no pristine body: skip the disk read.
                resp
            } else if let Some(data) = cache_read_metadata(state, coords).await {
                metadata_ok(state, coords, &response.entry, data, algo).await
            } else {
                error!("cache: lost metadata cache file for {coords}");
                state.metadata.invalidate(&key);
                plain_error(503, "metadata cache lost")
            }
        }
        code if (500..=599).contains(&code) => {
            // Upstream trouble: a cached copy beats failing the build. 4xx
            // stays forwarded — a 404 is a real answer, not an outage.
            if let Some(data) = cache_read_metadata(state, coords).await {
                warn!("proxy: upstream returned HTTP {code} for {coords}; serving cached metadata");
                let stale = cache_find_metadata(state, coords)
                    .await
                    .or(cached_entry)
                    .unwrap_or_else(MavenEntry::new);
                return metadata_ok(state, coords, &stale, data, algo).await;
            }
            warn!("fetch: upstream returned HTTP status {code} for {coords}");
            text_response(
                code,
                TEXT_CTYPE,
                String::from_utf8_lossy(&response.data).into_owned(),
            )
        }
        code => {
            warn!("fetch: upstream returned HTTP status {code} for {coords}");
            text_response(
                code,
                TEXT_CTYPE,
                String::from_utf8_lossy(&response.data).into_owned(),
            )
        }
    }
}
