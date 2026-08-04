//! The single Maven wildcard handler: classification, the metadata serve
//! ladder, generated checksums, and artifact downloads.

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Method, Uri},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::fs::{fetch_file, file_mtime, store_file};
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{etag_marker, filtered_etag, unmark_etag, Marker};
use chilled_core::http::{
    data_response, method_not_allowed, read_capped, text_response, FetchError,
};
use chilled_core::valid::decode_path_once;
use log::{debug, error, info, warn};

use crate::checksum::{split_checksum, ChecksumAlgo};
use crate::constants::{
    JAR_CTYPE, MAX_ARTIFACT_SIZE, MAX_METADATA_SIZE, MAX_PATH_LEN, MAX_SEGMENTS, METADATA_FILE,
    OCTET_CTYPE, TEXT_CTYPE, XML_CTYPE,
};
use crate::coords::MavenCoords;
use crate::filter;
use crate::model::MavenEntry;
use crate::probe;
use crate::sidecar::{VersionTimes, SIDECAR_FILE};
use crate::state::AppState;
use crate::valid::{is_artifact_file, is_dir_segment, is_file_segment, is_version, MavenRequest};

/// Builds a plain-text error response.
fn plain_error(status: u16, msg: &str) -> Response {
    text_response(status, TEXT_CTYPE, msg.to_owned())
}

/// Classifies a raw (still percent-encoded) request path. `None` means 404;
/// nothing here may reach upstream.
pub(crate) fn classify(raw_path: &str) -> Option<MavenRequest> {
    let decoded = decode_path_once(raw_path)?;
    let path = decoded.strip_prefix('/').unwrap_or(&decoded);
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return None;
    }

    let segs: Vec<&str> = path.split('/').collect();
    if segs.len() > MAX_SEGMENTS {
        return None;
    }
    let (file, dirs) = segs.split_last()?;
    if !dirs.iter().all(|s| is_dir_segment(s)) || !is_file_segment(file) {
        return None;
    }

    let (base, algo) = split_checksum(file);
    if base == METADATA_FILE {
        let parent = dirs.last()?;
        // A snapshot *version* directory, not an artifactId that merely ends in
        // `-SNAPSHOT` — versions start with a digit, so requiring that keeps
        // such an artifact on the gated path instead of passing it through.
        let snapshot_version_dir = parent.ends_with("-SNAPSHOT")
            && parent.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if snapshot_version_dir {
            // Central hosts no snapshots; snapshot version-dir metadata is
            // passed through ungated in v1.
            if segs.len() < 4 {
                return None;
            }
            debug!("proxy: snapshot metadata passes through ungated (v1 limitation): {path}");
            return Some(MavenRequest::SnapshotMetadata {
                rel: path.to_owned(),
            });
        }
        if segs.len() < 3 {
            return None;
        }
        let coords = MavenCoords::new(&dirs[..dirs.len() - 1], parent);
        return Some(MavenRequest::Metadata { coords, algo });
    }

    // Artifact download: {group...}/{artifact}/{version}/{file}.
    if segs.len() < 4 {
        return None;
    }
    let version = dirs[dirs.len() - 1];
    let artifact = dirs[dirs.len() - 2];
    if !is_version(version) || !is_artifact_file(artifact, version, file) {
        return None;
    }
    let coords = MavenCoords::new(&dirs[..dirs.len() - 2], artifact);
    Some(MavenRequest::Artifact {
        coords,
        version: version.to_owned(),
        file: (*file).to_owned(),
    })
}

/// Handles any request under the Maven mount.
pub(crate) async fn handle_maven(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return method_not_allowed();
    }
    let Some(request) = classify(uri.path()) else {
        warn!(
            "proxy: unrecognized or invalid repository path: {}",
            uri.path()
        );
        return plain_error(404, "not found");
    };

    match request {
        MavenRequest::Metadata { coords, algo } => {
            serve_metadata(&state, &coords, algo, &headers).await
        }
        MavenRequest::SnapshotMetadata { rel } => {
            let ctype = if rel.ends_with(".xml") {
                XML_CTYPE
            } else {
                TEXT_CTYPE
            };
            pass_through(&state, &rel, ctype).await
        }
        MavenRequest::Artifact {
            coords,
            version,
            file,
        } => serve_artifact(&state, &coords, &version, &file).await,
    }
}

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
fn sidecar_path(state: &AppState, coords: &MavenCoords) -> PathBuf {
    state
        .config
        .repo_dir
        .join(coords.dir_rel())
        .join(SIDECAR_FILE)
}

/// Attaches cooldown-aware validators: a weak marked ETag (and no
/// `Last-Modified`) when filtered, the upstream validators otherwise.
fn with_metadata_validators(
    mut builder: axum::http::response::Builder,
    entry: &MavenEntry,
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

/// Builds a metadata `304 Not Modified` response (no body).
fn metadata_not_modified(entry: &MavenEntry, state: &AppState, coords: &MavenCoords) -> Response {
    with_metadata_validators(
        Response::builder().status(304),
        entry,
        state.config.serve_marker(coords),
    )
    .body(Body::empty())
    .expect("valid 304 response")
}

/// Reads the pristine cached metadata file off the blocking thread pool.
async fn cache_read_metadata(state: &AppState, coords: &MavenCoords) -> Option<Vec<u8>> {
    let path = metadata_cache_path(state, coords);
    tokio::task::spawn_blocking(move || fetch_file(&path))
        .await
        .ok()
        .flatten()
}

/// Recreates metadata validators from the cache file's mtime.
async fn cache_find_metadata(state: &AppState, coords: &MavenCoords) -> Option<MavenEntry> {
    let path = metadata_cache_path(state, coords);
    tokio::task::spawn_blocking(move || file_mtime(&path))
        .await
        .ok()
        .flatten()
        .map(|mtime| {
            let mut entry = MavenEntry::new();
            entry.set_mtime(mtime);
            entry
        })
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

    // Pin identity encoding so the body is filterable bytes, never compressed.
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

    let data = read_capped(&mut response, MAX_METADATA_SIZE).await?;
    Ok(MetaResponse {
        entry,
        status,
        data,
    })
}

/// Serves artifact-level metadata (or a generated checksum of it).
async fn serve_metadata(
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
            let path = metadata_cache_path(state, coords);
            let data = response.data.clone();
            let mtime = response.entry.mtime();
            let _ = tokio::task::spawn_blocking(move || store_file(&path, &data, mtime)).await;
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
            } else if let Some(data) = cache_read_metadata(state, coords).await {
                metadata_ok(state, coords, &response.entry, data, algo).await
            } else {
                error!("cache: lost metadata cache file for {coords}");
                state.metadata.invalidate(&key);
                plain_error(503, "metadata cache lost")
            }
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
            None => with_metadata_validators(
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

    match algo {
        None => with_metadata_validators(
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

/// Parses versions, tops up the sidecar via POM probes, persists it, and
/// filters the metadata. `Ok(None)` means no version survived.
async fn filter_pipeline(
    state: &AppState,
    coords: &MavenCoords,
    data: Vec<u8>,
    cutoff: u64,
) -> Result<Option<Vec<u8>>, Response> {
    let side_path = sidecar_path(state, coords);

    // Parse the version list and load the sidecar off-thread.
    let parse_data = data.clone();
    let load_path = side_path.clone();
    let parsed = tokio::task::spawn_blocking(move || {
        filter::list_versions(&parse_data).map(|v| (v, VersionTimes::load(&load_path)))
    })
    .await;
    let (versions, mut times) = match parsed {
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
        filter::filter_metadata(&data, &times, cutoff)
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
async fn pass_through(state: &AppState, rel: &str, ctype: &str) -> Response {
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

    match read_capped(&mut response, MAX_METADATA_SIZE).await {
        Ok(data) => data_response(ctype, Bytes::from(data)),
        Err(FetchError::TooLarge) => plain_error(507, "upstream response too large"),
        Err(FetchError::Http(err)) => {
            error!("fetch: pass-through read failed for {rel}: {err}");
            plain_error(502, "upstream fetch failed")
        }
    }
}

// ---------------------------------------------------------------------------
// Artifact downloads.
// ---------------------------------------------------------------------------

/// Content type for an artifact file name.
fn ctype_for(file: &str) -> &'static str {
    let (base, algo) = split_checksum(file);
    if algo.is_some() {
        return TEXT_CTYPE;
    }
    if base.ends_with(".jar") {
        JAR_CTYPE
    } else if base.ends_with(".pom") || base.ends_with(".xml") {
        XML_CTYPE
    } else {
        OCTET_CTYPE
    }
}

/// The download gate's verdict for one version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Old enough to serve.
    Allow,
    /// Inside the cooldown window, or undatable — refuse (403).
    Refuse,
    /// Upstream does not carry this version at all — not found (404).
    NotFound,
}

impl From<bool> for Gate {
    fn from(old_enough: bool) -> Self {
        if old_enough {
            Gate::Allow
        } else {
            Gate::Refuse
        }
    }
}

/// Whether this version may be downloaded under `--restrict-downloads`.
///
/// **Fail-closed**: the sidecar age (probed on demand — Maven fetches pinned
/// artifacts without reading metadata first) must exist and be `<= cutoff`,
/// with the one exception that upstream reporting the version absent is a
/// definite answer and becomes a 404 rather than a refusal.
async fn artifact_old_enough(
    state: &AppState,
    coords: &MavenCoords,
    version: &str,
    cutoff: u64,
) -> Gate {
    let side_path = sidecar_path(state, coords);
    let load_path = side_path.clone();
    let Ok(mut times) = tokio::task::spawn_blocking(move || VersionTimes::load(&load_path)).await
    else {
        return Gate::Refuse;
    };

    // A first-seen guess is retried while it still gates, so a transient probe
    // failure does not refuse an old artifact for a whole window.
    if let Some(ts) = times.get(version) {
        if !(times.is_provisional(version) && ts > cutoff) {
            return Gate::from(ts <= cutoff);
        }
    }

    let stamp = match probe::probe_version(
        &state.client,
        &state.config.upstream_url,
        coords,
        version,
    )
    .await
    {
        probe::Probed::Stamped(stamp) => stamp,
        // Nothing to record: the version is not in this repository, so a
        // first-seen stamp would only pollute the sidecar with a version that
        // does not exist — and gate it for a window if it ever appears.
        probe::Probed::Absent => return Gate::NotFound,
    };
    let ts = stamp.ts;
    times.insert(version.to_owned(), stamp);
    let _ = tokio::task::spawn_blocking(move || times.save(&side_path)).await;
    Gate::from(ts <= cutoff)
}

/// Downloads an artifact file from upstream; errors come back as ready-made
/// responses to forward.
async fn fetch_artifact(state: &AppState, rel: &str) -> Result<Bytes, Response> {
    let url = state
        .config
        .upstream_url
        .join(rel)
        .expect("validated segments join onto the pinned upstream URL");

    let mut response = state.client.get(url).send().await.map_err(|err| {
        error!("fetch: artifact connection failed for {rel}: {err}");
        plain_error(502, "upstream fetch failed")
    })?;

    let status = response.status();
    if !status.is_success() {
        let code = status.as_u16();
        warn!("fetch: upstream returned HTTP status {code} for {rel}");
        let body = response.text().await.unwrap_or_default();
        return Err(text_response(code, TEXT_CTYPE, body));
    }

    match read_capped(&mut response, MAX_ARTIFACT_SIZE).await {
        Ok(data) => Ok(Bytes::from(data)),
        Err(FetchError::TooLarge) => Err(plain_error(507, "artifact too large")),
        Err(FetchError::Http(err)) => {
            error!("fetch: artifact read failed for {rel}: {err}");
            Err(plain_error(502, "upstream fetch failed"))
        }
    }
}

/// Serves an artifact file: restrict gate, disk cache, then upstream.
async fn serve_artifact(
    state: &AppState,
    coords: &MavenCoords,
    version: &str,
    file: &str,
) -> Response {
    // Fail-closed download gate, before any cache read.
    if state.config.settings.restrict_downloads {
        if let Some(cutoff) = state.config.cutoff_for(coords) {
            match artifact_old_enough(state, coords, version, cutoff).await {
                Gate::Allow => {}
                Gate::Refuse => {
                    warn!(
                        "download: refused {coords}:{version}: newer than cooldown or unverifiable"
                    );
                    return plain_error(403, "version is within the cooldown window");
                }
                Gate::NotFound => {
                    debug!("download: {coords}:{version} is not in this repository");
                    return plain_error(404, "not found");
                }
            }
        }
    }

    let rel = format!("{}/{version}/{file}", coords.dir_rel());
    let path = state.config.repo_dir.join(&rel);

    let read_path = path.clone();
    let cached = tokio::task::spawn_blocking(move || fetch_file(&read_path))
        .await
        .ok()
        .flatten();
    if let Some(data) = cached {
        info!("cache: served cached {rel} ({} bytes)", data.len());
        return data_response(ctype_for(file), Bytes::from(data));
    }

    match fetch_artifact(state, &rel).await {
        Ok(data) => {
            // Store off-thread; `Bytes` clones are cheap (refcounted).
            let stored = data.clone();
            let _ = tokio::task::spawn_blocking(move || store_file(&path, &stored, None)).await;
            info!("cache: stored new artifact {rel} ({} bytes)", data.len());
            data_response(ctype_for(file), data)
        }
        Err(response) => response,
    }
}
