//! PyPI route classification and handlers: `/simple/...` and `/files/...`.

#[cfg(test)]
mod tests;

use std::path::Path;

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, Uri},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{
    etag_format, etag_marker, filtered_etag, format_etag, rewrite_etag, unmark_etag,
};
use chilled_core::http::{
    data_response, error_response, method_not_allowed, read_capped, text_response, FetchError,
};
use chilled_core::valid::decode_path_once;
use log::{debug, error, info, warn};
use serde_json::Value;

use crate::accept::{negotiate, Format};
use crate::constants::{FILE_CTYPE, MAX_FILE_SIZE, MAX_SIMPLE_SIZE, SIMPLE_JSON_CTYPE, TEXT_CTYPE};
use crate::model::{cache_fetch_simple, cache_store_simple, cache_try_find_simple, PypiEntry};
use crate::state::AppState;
use crate::{filter, render, valid, Config};

/// A classified request path (already percent-decoded exactly once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    /// `GET /simple/` — the (empty) project list.
    ProjectList,
    /// `GET /simple/{normalized}/` — a project index.
    Project(String),
    /// 301 to the canonical `/simple/{normalized}/` form.
    Redirect(String),
    /// `GET /files/{project}/{fhp_path}` — a distribution file.
    File {
        project: String,
        fhp_path: String,
        filename: String,
    },
    NotFound,
}

/// Classifies a decoded request path into a [`Route`].
pub(crate) fn classify(path: &str) -> Route {
    if let Some(rest) = path.strip_prefix("/simple") {
        return classify_simple(rest);
    }
    if let Some(rest) = path.strip_prefix("/files/") {
        return classify_file(rest);
    }
    Route::NotFound
}

/// Classifies the remainder after `/simple` (empty, `/`, or `/{project}[/]`).
fn classify_simple(rest: &str) -> Route {
    if rest.is_empty() || rest == "/" {
        return Route::ProjectList;
    }
    let Some(rest) = rest.strip_prefix('/') else {
        // e.g. `/simplex` — not under the simple root.
        return Route::NotFound;
    };
    let (name, slashed) = match rest.strip_suffix('/') {
        Some(name) => (name, true),
        None => (rest, false),
    };
    if name.contains('/') || !valid::is_valid_name(name) {
        return Route::NotFound;
    }
    let normalized = valid::normalize(name);
    if !slashed || normalized != name {
        Route::Redirect(normalized)
    } else {
        Route::Project(normalized)
    }
}

/// Classifies the remainder after `/files/` (`{project}/{fhp_path}`).
fn classify_file(rest: &str) -> Route {
    let Some((project, fhp_path)) = rest.split_once('/') else {
        return Route::NotFound;
    };
    // The files route accepts only already-normalized names (no redirects).
    if !valid::is_valid_name(project) || valid::normalize(project) != project {
        return Route::NotFound;
    }
    let Some(filename) = valid::validate_fhp_path(fhp_path) else {
        return Route::NotFound;
    };
    Route::File {
        project: project.to_owned(),
        fhp_path: fhp_path.to_owned(),
        filename: filename.to_owned(),
    }
}

/// Handles every request under the `/pypi` mount.
pub(crate) async fn handle_pypi(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return method_not_allowed();
    }
    let raw = uri.path();
    let Some(path) = decode_path_once(raw) else {
        warn!("proxy: rejected undecodable request path: {raw}");
        return error_response(404);
    };

    match classify(&path) {
        Route::ProjectList => project_list(&headers),
        Route::Project(name) => serve_project(&state, &name, &headers).await,
        Route::Redirect(name) => redirect_to_project(&state.config, &name),
        Route::File {
            project,
            fhp_path,
            filename,
        } => serve_file(&state, &project, &fhp_path, &filename).await,
        Route::NotFound => {
            debug!("proxy: unrecognized request path: {path}");
            error_response(404)
        }
    }
}

/// Adds `Vary: Accept` to a content-negotiated response.
fn with_vary(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

/// Serves the minimal empty project list in the negotiated format.
fn project_list(headers: &HeaderMap) -> Response {
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
fn redirect_to_project(config: &Config, name: &str) -> Response {
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
fn entry_validator(entry: &PypiEntry) -> String {
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
async fn project_ok(
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
async fn cache_read_simple(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_fetch_simple(&dir, &name))
        .await
        .ok()
        .flatten()
}

/// Stores a pristine simple index off the blocking thread pool.
async fn cache_write_simple(dir: &Path, entry: &PypiEntry, data: &[u8]) {
    let dir = dir.to_path_buf();
    let entry = entry.clone();
    let data = data.to_vec();
    let _ = tokio::task::spawn_blocking(move || cache_store_simple(&dir, &entry, &data)).await;
}

/// Recreates entry metadata from the cache file's mtime off the blocking pool.
async fn cache_find_simple(dir: &Path, name: &str) -> Option<PypiEntry> {
    let dir = dir.to_path_buf();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || cache_try_find_simple(&dir, &name))
        .await
        .ok()
        .flatten()
}

/// Simple-index download result.
struct SimpleResponse {
    /// Entry plus updated response metadata (etag / last-modified).
    entry: PypiEntry,
    /// Upstream HTTP response status code.
    status: u16,
    /// Upstream `Content-Type` (empty when absent/unreadable).
    ctype: String,
    /// Upstream HTTP response body.
    data: Vec<u8>,
}

/// Whether an upstream `Content-Type` is the PEP 691 JSON simple type.
fn is_json_simple(ctype: &str) -> bool {
    ctype
        .split(';')
        .next()
        .is_some_and(|t| t.trim().eq_ignore_ascii_case(SIMPLE_JSON_CTYPE))
}

/// Downloads a project's simple index from upstream (always requesting PEP 691
/// JSON), sending the conditional-request headers carried by `entry`.
async fn download_simple(
    state: &AppState,
    mut entry: PypiEntry,
) -> Result<SimpleResponse, FetchError> {
    let url = state
        .config
        .upstream_url
        .join(&format!("{}/", entry.name()))
        .expect("valid normalized project URL");

    // Pin identity encoding so the cap and cache see the real bytes.
    let mut request = state
        .client
        .get(url)
        .header(header::ACCEPT, SIMPLE_JSON_CTYPE)
        .header(header::ACCEPT_ENCODING, "identity");
    if let Some(etag) = entry.etag() {
        request = request.header(header::IF_NONE_MATCH, etag);
    } else if let Some(last_modified) = entry.last_modified() {
        request = request.header(header::IF_MODIFIED_SINCE, last_modified);
    }

    let mut response = request.send().await.map_err(FetchError::Http)?;
    let status = response.status().as_u16();
    let ctype = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();

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

    let data = read_capped(&mut response, MAX_SIMPLE_SIZE).await?;

    Ok(SimpleResponse {
        entry,
        status,
        ctype,
        data,
    })
}

/// Passes a non-JSON upstream body through verbatim (no cooldown active).
fn passthrough_response(ctype: &str, data: Vec<u8>) -> Response {
    let ctype = HeaderValue::from_str(ctype)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::VARY, "Accept")
        .body(Body::from(data))
        .expect("valid passthrough response")
}

/// Fetches a project index from upstream (or stale cache) and serves it.
///
/// `window_ok` indicates the client's cached copy was produced at the same
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
        200 if !is_json_simple(&response.ctype) => {
            if state.config.cutoff_for(name).is_some() {
                error!(
                    "cooldown: upstream did not provide JSON simple index for {name}; \
                     refusing to serve ungated"
                );
                text_response(
                    502,
                    TEXT_CTYPE,
                    "upstream did not provide a JSON simple index\n".into(),
                )
            } else {
                warn!("proxy: passing through non-JSON simple index for {name}; file URLs are unproxied");
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
async fn serve_project(state: &AppState, name: &str, headers: &HeaderMap) -> Response {
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
/// (or unparseable) upload-time all refuse the download.
async fn file_old_enough(state: &AppState, project: &str, filename: &str, cutoff: u64) -> bool {
    let mut data = cache_read_simple(&state.config.simple_dir, project).await;

    // A client installing from a fully pinned lockfile may never request the
    // index, so fetch it on demand rather than refusing an old distribution.
    if data.is_none() {
        debug!("download: fetching simple index for {project} to age-check {filename}");
        if let Ok(response) = download_simple(state, PypiEntry::new(project)).await {
            if response.status == 200 && is_json_simple(&response.ctype) {
                cache_store_simple(&state.config.simple_dir, &response.entry, &response.data);
                state.metadata.store(project, response.entry.clone());
                data = Some(response.data);
            }
        }
    }

    let Some(data) = data else { return false };
    // A PEP 658 `.metadata` sidecar ages with its distribution.
    let filename = valid::distribution_name(filename).to_owned();
    let secs = tokio::task::spawn_blocking(move || -> Option<u64> {
        let doc: Value = serde_json::from_slice(&data).ok()?;
        let file = doc
            .get("files")?
            .as_array()?
            .iter()
            .find(|f| f.get("filename").and_then(Value::as_str) == Some(filename.as_str()))?;
        filter::parse_upload_time(file.get("upload-time")?.as_str()?)
    })
    .await
    .ok()
    .flatten();
    matches!(secs, Some(secs) if secs <= cutoff)
}

/// Downloads a distribution file from the pinned upstream files host.
///
/// On an upstream HTTP error or a transport failure, returns a ready-made
/// error `Response` to forward to the client.
async fn download_file(state: &AppState, fhp_path: &str, label: &str) -> Result<Bytes, Response> {
    let url = match state.config.files_url.join(fhp_path) {
        Ok(url) => url,
        Err(err) => {
            warn!("download: cannot build upstream file URL for {label}: {err}");
            return Err(error_response(404));
        }
    };

    let mut response = state.client.get(url).send().await.map_err(|err| {
        error!("fetch: file connection failed for {label}: {err}");
        text_response(502, TEXT_CTYPE, format!("upstream fetch failed: {err}\n"))
    })?;

    let status = response.status();
    if !status.is_success() {
        let code = status.as_u16();
        let body = response.text().await.unwrap_or_default();
        warn!("fetch: upstream returned HTTP status {code} for {label}");
        return Err(text_response(code, TEXT_CTYPE, body));
    }

    match read_capped(&mut response, MAX_FILE_SIZE).await {
        Ok(data) => Ok(Bytes::from(data)),
        Err(FetchError::TooLarge) => Err(error_response(507)),
        Err(FetchError::Http(err)) => Err(text_response(
            502,
            TEXT_CTYPE,
            format!("upstream fetch failed: {err}\n"),
        )),
    }
}

/// Handles `GET /files/{project}/{fhp_path}`.
async fn serve_file(state: &AppState, project: &str, fhp_path: &str, filename: &str) -> Response {
    let label = format!("{project}/{filename}");

    // With --restrict-downloads, refuse files newer than the cooldown even if
    // requested directly (fail-closed, before any cache read).
    if state.config.settings.restrict_downloads {
        if let Some(cutoff) = state.config.cutoff_for(project) {
            if !file_old_enough(state, project, filename, cutoff).await {
                warn!("download: refused {label}: newer than cooldown or unverifiable");
                return text_response(
                    403,
                    TEXT_CTYPE,
                    "download refused by cooldown policy\n".into(),
                );
            }
        }
    }

    let file_path = state.config.files_dir.join(project).join(filename);
    let cached = {
        let path = file_path.clone();
        tokio::task::spawn_blocking(move || chilled_core::cache::fs::fetch_file(&path))
            .await
            .ok()
            .flatten()
    };
    if let Some(data) = cached {
        info!("cache: served cached file {label} ({} bytes)", data.len());
        return data_response(FILE_CTYPE, Bytes::from(data));
    }

    match download_file(state, fhp_path, &label).await {
        Ok(data) => {
            // Store off-thread; `Bytes` clones are cheap (refcounted).
            let stored = data.clone();
            let _ = tokio::task::spawn_blocking(move || {
                chilled_core::cache::fs::store_file(&file_path, &stored, None);
            })
            .await;
            info!("cache: stored new file {label} ({} bytes)", data.len());
            data_response(FILE_CTYPE, data)
        }
        Err(response) => response,
    }
}
