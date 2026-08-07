//! The simple-index serve ladder: warm metadata, disk cache, then upstream.

use axum::{
    http::{header, HeaderMap},
    response::Response,
};
use chilled_core::etag::{etag_format, etag_marker, unmark_etag};
use chilled_core::http::{error_response, text_response};
use log::{debug, error, warn};

use crate::accept::{negotiate, Format};
use crate::constants::TEXT_CTYPE;
use crate::model::PypiEntry;
use crate::routes::pypi::cache::{cache_find_simple, cache_read_simple, cache_write_simple};
use crate::routes::pypi::fetch::{download_simple, is_json_simple, passthrough_response};
use crate::routes::pypi::respond::{
    accept_header, empty_index, project_memo_hit, project_not_modified, project_ok,
};
use crate::state::AppState;

/// Fetches a project index from upstream (or stale cache) and serves it.
///
/// `window_ok` indicates the client's cached copy was filtered at the same
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
            // Fail closed as an *empty index*, matching pypi's emptied-document
            // behavior: a resolver reads "no versions here" and falls through
            // to another index that can date the package, instead of aborting
            // the whole resolution on an error.
            warn!(
                "cooldown: simple index for {name} carries no upload times; withholding every \
                 file (resolvers will see no versions — serve {name} from an index that dates it)"
            );
            let empty = empty_index(name);
            project_ok(&response.entry, empty, state, name, fmt).await
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

            if window_ok && response.entry.meta.is_equivalent(&entry.meta) {
                project_not_modified(&response.entry, &state.config, name, fmt)
            } else {
                project_ok(&response.entry, response.data, state, name, fmt).await
            }
        }
        304 => {
            debug!("fetch: cached simple index for {name} is up to date");
            state.metadata.store(name, response.entry.clone());

            if window_ok && response.entry.meta.is_equivalent(&entry.meta) {
                project_not_modified(&response.entry, &state.config, name, fmt)
            } else if let Some(resp) = project_memo_hit(&response.entry, state, name, fmt) {
                // A memo hit needs no pristine body: skip the disk read.
                resp
            } else if let Some(data) = cache_read_simple(&state.config.simple_dir, name).await {
                project_ok(&response.entry, data, state, name, fmt).await
            } else {
                error!("cache: lost simple index cache file for {name}");
                state.metadata.invalidate(name);
                error_response(503)
            }
        }
        code if (500..=599).contains(&code) => {
            // Upstream trouble: a cached copy beats failing the install. 4xx
            // stays forwarded — a 404 is a real answer, not an outage.
            if let Some(data) = cache_read_simple(&state.config.simple_dir, name).await {
                warn!("proxy: upstream returned HTTP {code} for {name}; serving cached index");
                let stale = cache_find_simple(&state.config.simple_dir, name)
                    .await
                    .unwrap_or_else(|| PypiEntry::new(name));
                return project_ok(&stale, data, state, name, fmt).await;
            }
            warn!("fetch: upstream returned HTTP status {code} for {name}");
            text_response(
                code,
                TEXT_CTYPE,
                String::from_utf8_lossy(&response.data).into_owned(),
            )
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
        entry.meta.set_etag(&unmark_etag(inm));
        client_marker = etag_marker(inm);
        client_fmt = etag_format(inm);
    }
    let window_ok =
        client_marker == state.config.serve_marker(name) && client_fmt == Some(fmt.tag());

    // Serve from cache when the metadata cache is warm and unexpired.
    if let Some(cached_entry) = state.metadata.fetch(name) {
        if cached_entry
            .meta
            .is_expired_with_ttl(&state.config.settings.cache_ttl)
        {
            debug!("proxy: simple cache expired for {name}, refreshing...");
            return forward_project(state, entry, Some(cached_entry), name, window_ok, fmt).await;
        }

        if window_ok && cached_entry.meta.is_equivalent(&entry.meta) {
            debug!("proxy: simple metadata cache hit for {name}");
            return project_not_modified(&cached_entry, &state.config, name, fmt);
        }

        // A memo hit needs no pristine body, so skip the disk read entirely.
        if let Some(response) = project_memo_hit(&cached_entry, state, name, fmt) {
            debug!("proxy: simple memo hit for {name}");
            return response;
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
