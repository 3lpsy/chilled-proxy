//! The npm request handler: packuments, version docs, and tarball downloads.
//!
//! Registered as the router fallback so the raw URI path is classified with
//! exactly one percent-decode — axum's `Path` extractor would decode `%2f` and
//! make `/@scope%2fname` indistinguishable from `/@scope/name`.

#[cfg(test)]
mod tests;

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Method, Uri},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::fs::{fetch_file, file_mtime, store_file};
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{etag_marker, filtered_etag, rewrite_etag, unmark_etag, Marker};
use chilled_core::http::{
    data_response, error_response, json_response, method_not_allowed, read_capped, FetchError,
};
use chilled_core::time::parse_rfc3339z;
use chilled_core::valid::decode_path_once;
use log::{debug, error, info, warn};

use crate::constants::{MAX_PACKUMENT_SIZE, MAX_TARBALL_SIZE, PACKUMENT_CTYPE, TARBALL_CTYPE};
use crate::filter::{self, FilterResult};
use crate::http::format_npm_error;
use crate::model::{NpmEntry, PackageRef};
use crate::state::AppState;
use crate::valid;
use crate::Config;

/// A classified npm request (after exactly one percent-decode).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NpmRequest {
    /// Full package document.
    Packument(PackageRef),
    /// Single version document, derived from the filtered packument.
    VersionDoc(PackageRef, String),
    /// Tarball download: the file (`{unscoped}-{version}.tgz`) and its version.
    Tarball(PackageRef, String, String),
}

/// Decodes a raw request path exactly once and classifies it.
pub(crate) fn parse_request(raw_path: &str) -> Option<NpmRequest> {
    let raw = raw_path.strip_prefix('/').unwrap_or(raw_path);
    classify(&decode_path_once(raw)?)
}

/// Classifies a decoded path: packument, version doc, or tarball.
fn classify(path: &str) -> Option<NpmRequest> {
    let segments: Vec<&str> = path.split('/').collect();
    let (pkg, rest) = if let Some(scope) = segments[0].strip_prefix('@') {
        if segments.len() < 2 {
            return None;
        }
        (PackageRef::new(Some(scope), segments[1])?, &segments[2..])
    } else {
        (PackageRef::new(None, segments[0])?, &segments[1..])
    };
    match rest {
        [] => Some(NpmRequest::Packument(pkg)),
        [version] if valid::is_version(version) => {
            Some(NpmRequest::VersionDoc(pkg, (*version).to_owned()))
        }
        ["-", file] => {
            let version = valid::tarball_version(pkg.unscoped(), file)?;
            Some(NpmRequest::Tarball(pkg, (*file).to_owned(), version))
        }
        _ => None,
    }
}

/// Handles every npm request under the mount prefix.
pub(crate) async fn handle_npm(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return method_not_allowed();
    }
    let Some(request) = parse_request(uri.path()) else {
        warn!("proxy: malformed npm path: {}", uri.path());
        return error_response(404);
    };
    match request {
        NpmRequest::Packument(pkg) => handle_packument(&state, &headers, &pkg).await,
        NpmRequest::VersionDoc(pkg, version) => handle_version_doc(&state, &pkg, &version).await,
        NpmRequest::Tarball(pkg, file, version) => {
            handle_tarball(&state, &pkg, &file, &version).await
        }
    }
}

// --- Packuments ---

/// Outcome of the packument serve ladder.
enum Served {
    /// The client's cached copy is valid at the current window.
    NotModified(NpmEntry),
    /// Filtered+rewritten packument body, ready to wrap in a response.
    Body(NpmEntry, Bytes),
    /// A ready-made error or forwarded response.
    Done(Response),
}

/// Packument download result.
struct PackumentResponse {
    /// Entry with updated response metadata (etag / last-modified).
    entry: NpmEntry,
    /// Upstream HTTP response status code.
    status: u16,
    /// Upstream HTTP response body.
    data: Vec<u8>,
}

/// The source-content validator used as a memo key and for the weak ETag.
fn entry_validator(entry: &NpmEntry) -> String {
    entry
        .etag()
        .map(ToOwned::to_owned)
        .or_else(|| entry.last_modified())
        .unwrap_or_default()
}

/// Attaches the cooldown-aware ETag. Served bodies are always rewritten, so
/// the marker is `.cd<secs>-<bucket>` when filtered and `.rw` otherwise — and
/// the upstream `Last-Modified` is never emitted.
fn with_packument_validators(
    mut builder: axum::http::response::Builder,
    entry: &NpmEntry,
    marker: Option<Marker>,
) -> axum::http::response::Builder {
    if let Some(etag) = entry.etag() {
        let marked = match marker {
            Some(marker) => filtered_etag(etag, marker),
            None => rewrite_etag(etag),
        };
        builder = builder.header(header::ETAG, marked);
    }
    builder
}

/// Builds a packument `304 Not Modified` response (no body).
fn packument_not_modified(entry: &NpmEntry, config: &Config, name: &str) -> Response {
    with_packument_validators(
        Response::builder().status(304),
        entry,
        config.serve_marker(name),
    )
    .body(Body::empty())
    .expect("valid 304 response")
}

/// Handles a packument request: `GET /{name}`.
async fn handle_packument(state: &AppState, headers: &HeaderMap, pkg: &PackageRef) -> Response {
    let name = pkg.full_name();

    // Undo our ETag marker so the upstream conditional GET uses the real
    // validator; remember the marker — a 304 is only safe when its window and
    // cutoff bucket both match what we serve at now.
    let mut client_entry = NpmEntry::new();
    let mut client_marker = None;
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        client_entry.set_etag(&unmark_etag(inm));
        client_marker = etag_marker(inm);
    }
    let window_ok = client_marker == state.config.serve_marker(&name);

    match serve_packument(state, pkg, client_entry, window_ok).await {
        Served::NotModified(entry) => packument_not_modified(&entry, &state.config, &name),
        Served::Body(entry, body) => with_packument_validators(
            Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, PACKUMENT_CTYPE),
            &entry,
            state.config.serve_marker(&name),
        )
        .body(Body::from(body))
        .expect("valid packument response"),
        Served::Done(response) => response,
    }
}

/// The packument serve ladder: warm metadata, disk cache, then upstream.
async fn serve_packument(
    state: &AppState,
    pkg: &PackageRef,
    client_entry: NpmEntry,
    window_ok: bool,
) -> Served {
    let name = pkg.full_name();

    if let Some(cached_entry) = state.metadata.fetch(&name) {
        if cached_entry.is_expired_with_ttl(&state.config.settings.cache_ttl) {
            debug!("proxy: packument cache expired for {name}, refreshing...");
            return forward_packument(state, pkg, client_entry, Some(cached_entry), window_ok)
                .await;
        }

        if window_ok && cached_entry.is_equivalent(&client_entry) {
            debug!("proxy: packument metadata cache hit for {name}");
            return Served::NotModified(cached_entry);
        }

        if let Some(data) = cache_read_packument(state, pkg).await {
            debug!("proxy: packument data cache hit for {name}");
            return filtered_served(state, pkg, cached_entry, data).await;
        }
    }

    // Recreate metadata from the cache file mtime, then fetch from upstream.
    let mtimed_entry = cache_find_packument(state, pkg).await;
    forward_packument(state, pkg, client_entry, mtimed_entry, window_ok).await
}

/// Fetches a packument from upstream (or stale cache) and prepares serving.
async fn forward_packument(
    state: &AppState,
    pkg: &PackageRef,
    client_entry: NpmEntry,
    cached_entry: Option<NpmEntry>,
    window_ok: bool,
) -> Served {
    // Revalidate only against our own cached metadata. Forwarding the client's
    // validator with nothing cached would turn an upstream 304 into a 503 we
    // could never escape.
    let req_entry = cached_entry.unwrap_or_default();

    let response = match download_packument(state, req_entry, pkg).await {
        Ok(response) => response,
        Err(err) => {
            // Transport failure: serve a possibly-stale cached copy if present.
            if let Some(data) = cache_read_packument(state, pkg).await {
                warn!("proxy: forwarding possibly stale cached packument for {pkg}: {err}");
                // Label the stale body with validators derived from the cache
                // file, never the client's — they describe a different body.
                let stale = cache_find_packument(state, pkg).await.unwrap_or_default();
                return filtered_served(state, pkg, stale, data).await;
            }
            error!("fetch: packument connection failed for {pkg}: {err}");
            return Served::Done(json_response(502, format_npm_error(err)));
        }
    };

    match response.status {
        200 => {
            debug!("fetch: successfully got packument for {pkg}");
            cache_write_packument(state, pkg, &response.entry, &response.data).await;
            state
                .metadata
                .store(&pkg.full_name(), response.entry.clone());

            if window_ok && response.entry.is_equivalent(&client_entry) {
                Served::NotModified(response.entry)
            } else {
                filtered_served(state, pkg, response.entry, response.data).await
            }
        }
        304 => {
            debug!("fetch: cached packument for {pkg} is up to date");
            state
                .metadata
                .store(&pkg.full_name(), response.entry.clone());

            if window_ok && response.entry.is_equivalent(&client_entry) {
                Served::NotModified(response.entry)
            } else if let Some(data) = cache_read_packument(state, pkg).await {
                filtered_served(state, pkg, response.entry, data).await
            } else {
                error!("cache: lost packument cache file for {pkg}");
                state.metadata.invalidate(&pkg.full_name());
                Served::Done(error_response(503))
            }
        }
        code => {
            // Forward other upstream statuses (e.g. 404) verbatim.
            warn!("fetch: upstream returned HTTP status {code} for {pkg}");
            Served::Done(json_response(
                code,
                String::from_utf8_lossy(&response.data).into_owned(),
            ))
        }
    }
}

/// Filters+rewrites pristine packument bytes (memoized) into a serve outcome.
async fn filtered_served(
    state: &AppState,
    pkg: &PackageRef,
    entry: NpmEntry,
    data: Vec<u8>,
) -> Served {
    let name = pkg.full_name();
    let cutoff = state.config.cutoff_for(&name);
    let bucket = cutoff.map_or(0, |c| c / MEMO_BUCKET_SECS);
    let validator = entry_validator(&entry);

    if let Some(cached) = state.memo.get(&name, &validator, bucket) {
        return Served::Body(entry, cached);
    }

    // Filter off the async workers; packuments can be large.
    let proxy_url = state.config.settings.proxy_url.clone();
    let task_name = name.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        filter::filter_bytes(&data, cutoff, &proxy_url, &task_name)
    })
    .await;

    match outcome {
        Ok(FilterResult::Body(body)) => {
            state.memo.put(name, validator, bucket, body.clone());
            Served::Body(entry, body)
        }
        Ok(FilterResult::AllFiltered) => {
            info!("cooldown: every version of {name} is within the cooldown window");
            Served::Done(json_response(404, format_npm_error("Not found")))
        }
        Ok(FilterResult::Invalid) => {
            error!("proxy: upstream packument for {name} is not valid JSON");
            Served::Done(json_response(
                502,
                format_npm_error("upstream packument is not valid JSON"),
            ))
        }
        // A panic in the filter must not become an empty 200.
        Err(err) => {
            error!("cooldown: packument filter task failed for {name}: {err}");
            Served::Done(error_response(500))
        }
    }
}

/// Downloads a full packument, sending the conditional headers from `entry`.
async fn download_packument(
    state: &AppState,
    mut entry: NpmEntry,
    pkg: &PackageRef,
) -> Result<PackumentResponse, FetchError> {
    // Charset-validated names cannot break `Url::join`.
    let url = state
        .config
        .upstream_url
        .join(&pkg.upstream_packument_rel())
        .expect("validated packument URL");

    // Full doc only — the abbreviated "corgi" form lacks the `time` map needed
    // for cooldown; identity encoding because we parse the body.
    let mut request = state
        .client
        .get(url)
        .header(header::ACCEPT, PACKUMENT_CTYPE)
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

    let data = read_capped(&mut response, MAX_PACKUMENT_SIZE).await?;

    Ok(PackumentResponse {
        entry,
        status,
        data,
    })
}

// --- Version docs ---

/// Handles a version doc request (`GET /{name}/{version}`), derived from the
/// filtered packument so hidden versions stay hidden.
async fn handle_version_doc(state: &AppState, pkg: &PackageRef, version: &str) -> Response {
    match serve_packument(state, pkg, NpmEntry::new(), false).await {
        Served::Body(_, body) => {
            let version = version.to_owned();
            let extracted =
                tokio::task::spawn_blocking(move || extract_version_doc(&body, &version)).await;
            match extracted {
                Ok(Some(doc)) => json_response(200, doc),
                Ok(None) => json_response(404, format_npm_error("Not found")),
                Err(err) => {
                    error!("proxy: version doc task failed for {pkg}: {err}");
                    error_response(500)
                }
            }
        }
        Served::Done(response) => response,
        // Unreachable: version doc requests carry no client validators.
        Served::NotModified(_) => error_response(500),
    }
}

/// Extracts one version object from a serialized (filtered) packument.
fn extract_version_doc(body: &[u8], version: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_slice(body).ok()?;
    let versions = doc.get("versions")?;
    // npm also resolves dist-tags here (`GET /pkg/latest`). The tags were
    // already repaired by the filter, so a tag can only name a served version.
    let entry = versions.get(version).or_else(|| {
        let target = doc.get("dist-tags")?.get(version)?.as_str()?;
        versions.get(target)
    })?;
    serde_json::to_string(entry).ok()
}

// --- Tarballs ---

/// Handles a tarball download: `GET /{name}/-/{file}.tgz`.
async fn handle_tarball(state: &AppState, pkg: &PackageRef, file: &str, version: &str) -> Response {
    // With --restrict-downloads, refuse versions newer than the cooldown even
    // when requested directly (e.g. a poisoned lockfile).
    if state.config.settings.restrict_downloads {
        if let Some(cutoff) = state.config.cutoff_for(&pkg.full_name()) {
            if !tarball_old_enough(state, pkg, version, cutoff).await {
                warn!("download: refused {pkg}@{version}: newer than cooldown or unverifiable");
                return json_response(
                    403,
                    format_npm_error("cooldown: version not old enough or unverifiable"),
                );
            }
        }
    }

    if let Some(data) = cache_read_tarball(state, pkg, file).await {
        info!(
            "cache: served cached tarball {pkg}/{file} ({} bytes)",
            data.len()
        );
        return data_response(TARBALL_CTYPE, Bytes::from(data));
    }

    match download_tarball(state, pkg, file).await {
        Ok(data) => {
            // Store off-thread; `Bytes` clones are cheap (refcounted).
            let path = state.config.tarballs_dir.join(pkg.tarball_rel(file));
            let stored = data.clone();
            let _ = tokio::task::spawn_blocking(move || store_file(&path, &stored, None)).await;
            info!(
                "cache: stored new tarball {pkg}/{file} ({} bytes)",
                data.len()
            );
            data_response(TARBALL_CTYPE, data)
        }
        Err(response) => response,
    }
}

/// Whether `version` may be downloaded under `--restrict-downloads`.
///
/// The publish time is read from the locally cached *pristine* packument.
/// **Fail-closed**: no cached packument, unknown version, or a too-new stamp
/// all refuse the download.
async fn tarball_old_enough(
    state: &AppState,
    pkg: &PackageRef,
    version: &str,
    cutoff: u64,
) -> bool {
    let mut data = cache_read_packument(state, pkg).await;

    // `npm ci` installs straight from a lockfile without ever fetching the
    // packument, so on a cold cache there would be nothing to check against.
    // Fetch it on demand rather than refusing an otherwise-old version.
    if data.is_none() {
        debug!("download: fetching packument for {pkg} to age-check {version}");
        if let Ok(response) = download_packument(state, NpmEntry::new(), pkg).await {
            if response.status == 200 {
                cache_write_packument(state, pkg, &response.entry, &response.data).await;
                state
                    .metadata
                    .store(&pkg.full_name(), response.entry.clone());
                data = Some(response.data);
            }
        }
    }

    let Some(data) = data else { return false };
    let version = version.to_owned();
    let pubtime = tokio::task::spawn_blocking(move || {
        let doc: serde_json::Value = serde_json::from_slice(&data).ok()?;
        parse_rfc3339z(doc.get("time")?.get(&version)?.as_str()?)
    })
    .await
    .ok()
    .flatten();
    matches!(pubtime, Some(pt) if pt <= cutoff)
}

/// Downloads a tarball from upstream. On an upstream HTTP error or transport
/// failure, returns a ready-made error `Response` to forward to the client.
async fn download_tarball(
    state: &AppState,
    pkg: &PackageRef,
    file: &str,
) -> Result<Bytes, Response> {
    let url = state
        .config
        .upstream_url
        .join(&pkg.upstream_tarball_rel(file))
        .expect("validated tarball URL");

    let mut response = state.client.get(url).send().await.map_err(|e| {
        error!("fetch: tarball connection failed for {pkg}: {e}");
        json_response(502, format_npm_error(e))
    })?;

    let status = response.status();
    if !status.is_success() {
        let code = status.as_u16();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| format_npm_error("upstream error"));
        warn!("fetch: upstream returned HTTP status {code} for {pkg}/{file}");
        return Err(json_response(code, body));
    }

    match read_capped(&mut response, MAX_TARBALL_SIZE).await {
        Ok(data) => Ok(Bytes::from(data)),
        Err(FetchError::TooLarge) => Err(error_response(507)),
        Err(FetchError::Http(e)) => {
            error!("fetch: tarball read failed for {pkg}: {e}");
            Err(json_response(502, format_npm_error(e)))
        }
    }
}

// --- Disk cache plumbing (blocking FS off the async workers) ---

/// Reads the cached pristine packument off the blocking thread pool.
async fn cache_read_packument(state: &AppState, pkg: &PackageRef) -> Option<Vec<u8>> {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    tokio::task::spawn_blocking(move || fetch_file(&path))
        .await
        .ok()
        .flatten()
}

/// Stores the pristine packument, pinning its mtime to `Last-Modified`.
async fn cache_write_packument(state: &AppState, pkg: &PackageRef, entry: &NpmEntry, data: &[u8]) {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    let mtime = entry.mtime();
    let data = data.to_vec();
    let _ = tokio::task::spawn_blocking(move || store_file(&path, &data, mtime)).await;
}

/// Recreates packument metadata from the cache file's mtime.
async fn cache_find_packument(state: &AppState, pkg: &PackageRef) -> Option<NpmEntry> {
    let path = state.config.packuments_dir.join(pkg.packument_rel());
    let mtime = tokio::task::spawn_blocking(move || file_mtime(&path))
        .await
        .ok()
        .flatten()?;
    let mut entry = NpmEntry::new();
    entry.set_mtime(mtime);
    Some(entry)
}

/// Reads a cached tarball off the blocking thread pool.
async fn cache_read_tarball(state: &AppState, pkg: &PackageRef, file: &str) -> Option<Vec<u8>> {
    let path = state.config.tarballs_dir.join(pkg.tarball_rel(file));
    tokio::task::spawn_blocking(move || fetch_file(&path))
        .await
        .ok()
        .flatten()
}
