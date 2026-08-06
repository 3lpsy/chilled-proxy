//! The axum entry point for `GET /index/<path>`.

use axum::{
    extract::{Path as UrlPath, State},
    http::{header, HeaderMap},
    response::Response,
};
use chilled_core::etag::{etag_marker, unmark_etag};
use chilled_core::http::{error_response, json_response};
use log::{debug, warn};

use crate::cache::IndexEntry;

use crate::routes::index::fetch::forward_index;
use crate::routes::index::serve::{
    cache_find_index, cache_read_index, gen_config_json_file, index_memo_hit, index_not_modified,
    index_ok, CONFIG_JSON_ENDPOINT,
};
use crate::state::AppState;

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
        index_entry.meta.set_etag(&unmark_etag(inm));
        client_marker = etag_marker(inm);
    } else if let Some(ims) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
    {
        index_entry.meta.set_last_modified(ims);
    }

    let window_ok = client_marker == state.config.serve_marker(&name);

    // Serve from cache when the metadata cache is warm and unexpired.
    if let Some(cached_entry) = state.metadata.fetch(&name) {
        if cached_entry
            .meta
            .is_expired_with_ttl(&state.config.settings.cache_ttl)
        {
            debug!("proxy: index cache expired for {name}, refreshing...");
            return forward_index(&state, index_entry, Some(cached_entry), &name, window_ok).await;
        }

        if window_ok && cached_entry.meta.is_equivalent(&index_entry.meta) {
            debug!("proxy: index metadata cache hit for {name}");
            return index_not_modified(&cached_entry, &state.config, &name);
        }

        // A memo hit needs no pristine body, so skip the disk read entirely.
        if let Some(response) = index_memo_hit(&cached_entry, &state, &name) {
            debug!("proxy: index memo hit for {name}");
            return response;
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
