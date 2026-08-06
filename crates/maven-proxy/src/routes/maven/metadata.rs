//! Serving `maven-metadata.xml`: validators, the cache/upstream ladder, the
//! cooldown filter pipeline, and generated checksums.

use std::path::PathBuf;

use axum::{body::Body, http::header, http::HeaderMap, response::Response};
use bytes::Bytes;
use chilled_core::cache::fs::{fetch_file_async, file_mtime_async, store_file_async};
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{cooldown_validators, etag_marker, unmark_etag, Marker};
use chilled_core::http::{conditional_get, data_response, read_capped, text_response, FetchError};
use log::{debug, error, info, warn};

use crate::checksum::ChecksumAlgo;
use crate::constants::{TEXT_CTYPE, XML_CTYPE};
use crate::coords::MavenCoords;
use crate::filter;
use crate::model::MavenEntry;
use crate::probe;
use crate::routes::maven::handler::plain_error;
use crate::sidecar::{VersionTimes, SIDECAR_FILE};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Metadata serve ladder (mirrors the crates-proxy index route).
// ---------------------------------------------------------------------------

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
pub(super) fn sidecar_path(state: &AppState, coords: &MavenCoords) -> PathBuf {
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
pub(super) async fn serve_metadata(
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

/// Produces the filtered (or pristine) metadata body and serves it as XML or
/// as a generated checksum. The single source of truth for both routes keeps
/// `maven-metadata.xml.{algo}` coherent with the served `.xml` bytes.
async fn metadata_ok(
    state: &AppState,
    coords: &MavenCoords,
    entry: &MavenEntry,
    data: Vec<u8>,
    algo: Option<ChecksumAlgo>,
) -> Response {
    let Some(cutoff) = state.config.cutoff_for(coords) else {
        // Unfiltered: serve verbatim with the upstream validators.
        return match algo {
            None => cooldown_validators(
                Response::builder()
                    .status(200)
                    .header(header::CONTENT_TYPE, XML_CTYPE),
                entry,
                None,
            )
            .body(Body::from(data))
            .expect("valid metadata response"),
            // Rare race: cooldown vanished after routing; hash the pristine body.
            Some(algo) => text_response(200, TEXT_CTYPE, algo.hex(&data)),
        };
    };

    let bucket = cutoff / MEMO_BUCKET_SECS;
    let validator = entry.validator();
    let key = coords.dir_rel();

    // The sidecar also shapes the output, but only ever monotonically (versions
    // aging in); the hour-granular bucket accepts that ≤1h staleness.
    let body = if let Some(cached) = state.memo.get(&key, &validator, bucket) {
        cached
    } else {
        match filter_pipeline(state, coords, data, cutoff).await {
            Ok(Some(filtered)) => {
                let filtered = Bytes::from(filtered);
                state.memo.put(key, validator, bucket, filtered.clone());
                filtered
            }
            Ok(None) => {
                info!("cooldown: all versions of {coords} are within the cooldown window");
                return plain_error(404, "no versions outside the cooldown window");
            }
            Err(response) => return response,
        }
    };

    filtered_response(state, coords, entry, body, algo, bucket).await
}

/// Serves an already-produced filtered body as XML or a generated checksum.
async fn filtered_response(
    state: &AppState,
    coords: &MavenCoords,
    entry: &MavenEntry,
    body: Bytes,
    algo: Option<ChecksumAlgo>,
    bucket: u64,
) -> Response {
    match algo {
        None => cooldown_validators(
            Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, XML_CTYPE),
            entry,
            Some(Marker {
                window: state.config.settings.cooldown.as_secs(),
                bucket,
            }),
        )
        .body(Body::from(body))
        .expect("valid metadata response"),
        Some(algo) => {
            // Hash off the async workers; bodies can reach the metadata cap.
            match tokio::task::spawn_blocking(move || algo.hex(&body)).await {
                Ok(hex) => text_response(200, TEXT_CTYPE, hex),
                Err(err) => {
                    error!("cooldown: checksum task failed for {coords}: {err}");
                    plain_error(500, "internal error")
                }
            }
        }
    }
}

/// Serves the memoized filtered body for `entry` without touching the disk
/// cache — `None` on a memo miss, or when the artifact is unfiltered (the
/// verbatim pristine body is needed then).
async fn metadata_memo_hit(
    state: &AppState,
    coords: &MavenCoords,
    entry: &MavenEntry,
    algo: Option<ChecksumAlgo>,
) -> Option<Response> {
    let cutoff = state.config.cutoff_for(coords)?;
    let bucket = cutoff / MEMO_BUCKET_SECS;
    let body = state
        .memo
        .get(&coords.dir_rel(), &entry.validator(), bucket)?;
    Some(filtered_response(state, coords, entry, body, algo, bucket).await)
}

/// Parses versions, tops up the sidecar via POM probes, persists it, and
/// filters the metadata. `Ok(None)` means no version survived.
async fn filter_pipeline(
    state: &AppState,
    coords: &MavenCoords,
    data: Vec<u8>,
    cutoff: u64,
) -> Result<Option<Vec<u8>>, Response> {
    let side_path = sidecar_path(state, coords);

    // Parse the version list and load the sidecar off-thread. The body moves
    // through the task and back rather than being cloned for it.
    let load_path = side_path.clone();
    let parsed = tokio::task::spawn_blocking(move || {
        let versions = filter::list_versions(&data)?;
        Ok::<_, String>((data, versions, VersionTimes::load(&load_path)))
    })
    .await;
    let (data, versions, mut times) = match parsed {
        Ok(Ok(parsed)) => parsed,
        Ok(Err(err)) => {
            error!("cooldown: unparseable upstream metadata for {coords}: {err}");
            return Err(plain_error(502, "unparseable upstream metadata"));
        }
        Err(err) => {
            error!("cooldown: metadata parse task failed for {coords}: {err}");
            return Err(plain_error(500, "internal error"));
        }
    };

    let changed = probe::probe_versions(
        &state.client,
        &state.config.upstream_url,
        coords,
        &versions,
        &mut times,
        cutoff,
    )
    .await;

    // Persist the sidecar and filter off-thread. A panic must not become an
    // empty 200.
    let filter_coords = coords.to_string();
    let filtered = tokio::task::spawn_blocking(move || {
        if changed {
            times.save(&side_path);
        }
        filter::filter_metadata(&data, &versions, &times, cutoff)
    })
    .await;
    match filtered {
        Ok(Ok(filtered)) => Ok(filtered),
        Ok(Err(err)) => {
            error!("cooldown: metadata filter failed for {filter_coords}: {err}");
            Err(plain_error(502, "unparseable upstream metadata"))
        }
        Err(err) => {
            error!("cooldown: metadata filter task failed for {filter_coords}: {err}");
            Err(plain_error(500, "internal error"))
        }
    }
}

// ---------------------------------------------------------------------------
// Pass-through (snapshot metadata, unfiltered metadata checksums).
// ---------------------------------------------------------------------------

/// Fetches `rel` from upstream and forwards it verbatim, uncached.
pub(super) async fn pass_through(state: &AppState, rel: &str, ctype: &str) -> Response {
    let url = state
        .config
        .upstream_url
        .join(rel)
        .expect("validated segments join onto the pinned upstream URL");

    let mut response = match state.client.get(url).send().await {
        Ok(response) => response,
        Err(err) => {
            error!("fetch: pass-through connection failed for {rel}: {err}");
            return plain_error(502, "upstream fetch failed");
        }
    };

    let status = response.status().as_u16();
    if !response.status().is_success() {
        warn!("fetch: upstream returned HTTP status {status} for {rel}");
        let body = response.text().await.unwrap_or_default();
        return text_response(status, TEXT_CTYPE, body);
    }

    match read_capped(&mut response, state.config.settings.max_metadata_size).await {
        Ok(data) => data_response(ctype, Bytes::from(data)),
        Err(FetchError::TooLarge) => plain_error(507, "upstream response too large"),
        Err(FetchError::Http(err)) => {
            error!("fetch: pass-through read failed for {rel}: {err}");
            plain_error(502, "upstream fetch failed")
        }
    }
}
