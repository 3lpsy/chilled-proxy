//! Packument serving: validators, the cache/upstream ladder, and filtering.

use axum::{
    body::Body,
    http::{header, HeaderMap},
    response::Response,
};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{etag_marker, filtered_etag, rewrite_etag, unmark_etag, Marker};
use chilled_core::http::{error_response, json_response};
use log::{debug, error, info, warn};

use crate::constants::PACKUMENT_CTYPE;
use crate::filter::{self, FilterResult};
use crate::http::format_npm_error;
use crate::model::{NpmEntry, PackageRef};
use crate::routes::npm::cache::{
    cache_find_packument, cache_read_packument, cache_write_packument,
};
use crate::routes::npm::fetch::download_packument;
use crate::state::AppState;
use crate::Config;

pub(super) enum Served {
    /// The client's cached copy is valid at the current window.
    NotModified(NpmEntry),
    /// Filtered+rewritten packument body, ready to wrap in a response.
    Body(NpmEntry, Bytes),
    /// A ready-made error or forwarded response.
    Done(Response),
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
pub(super) async fn handle_packument(
    state: &AppState,
    headers: &HeaderMap,
    pkg: &PackageRef,
) -> Response {
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
pub(super) async fn serve_packument(
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

        // A memo hit needs no pristine body, so skip the disk read entirely.
        if let Some(served) = memoized_served(state, pkg, &cached_entry) {
            debug!("proxy: packument memo hit for {name}");
            return served;
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
            } else if let Some(served) = memoized_served(state, pkg, &response.entry) {
                // A memo hit needs no pristine body: skip the disk read.
                served
            } else if let Some(data) = cache_read_packument(state, pkg).await {
                filtered_served(state, pkg, response.entry, data).await
            } else {
                error!("cache: lost packument cache file for {pkg}");
                state.metadata.invalidate(&pkg.full_name());
                Served::Done(error_response(503))
            }
        }
        code if (500..=599).contains(&code) => {
            // Upstream trouble: a cached copy beats failing the install. 4xx
            // stays forwarded — a 404 is a real answer, not an outage.
            if let Some(data) = cache_read_packument(state, pkg).await {
                warn!("proxy: upstream returned HTTP {code} for {pkg}; serving cached packument");
                let stale = cache_find_packument(state, pkg).await.unwrap_or_default();
                return filtered_served(state, pkg, stale, data).await;
            }
            warn!("fetch: upstream returned HTTP status {code} for {pkg}");
            Served::Done(json_response(
                code,
                String::from_utf8_lossy(&response.data).into_owned(),
            ))
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

/// The memoized filtered+rewritten body for `entry`, served without touching
/// the disk cache — `None` on a memo miss.
fn memoized_served(state: &AppState, pkg: &PackageRef, entry: &NpmEntry) -> Option<Served> {
    let name = pkg.full_name();
    let bucket = state
        .config
        .cutoff_for(&name)
        .map_or(0, |c| c / MEMO_BUCKET_SECS);
    let body = state.memo.get(&name, &entry.validator(), bucket)?;
    Some(Served::Body(entry.clone(), body))
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
    let validator = entry.validator();

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
