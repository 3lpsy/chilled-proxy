//! Fetching an index entry upstream and the cache/upstream serve ladder.

use axum::response::Response;
use chilled_core::http::{conditional_get, error_response, json_response, FetchError};
use log::{debug, error, warn};

use crate::cache::IndexEntry;
use crate::http::format_json_error;
use crate::routes::index::serve::{
    cache_find_index, cache_read_index, cache_write_index, index_memo_hit, index_not_modified,
    index_ok,
};
use crate::state::AppState;

/// Registry index entry download result.
pub(crate) struct IndexResponse {
    /// Index entry plus updated response metadata (etag / last-modified).
    pub(crate) entry: IndexEntry,
    /// Upstream HTTP response status code.
    pub(crate) status: u16,
    /// Upstream HTTP response body.
    pub(crate) data: Vec<u8>,
}

pub(crate) async fn download_index_entry(
    state: &AppState,
    mut entry: IndexEntry,
) -> Result<IndexResponse, FetchError> {
    let url = state.config.index_url.join(&entry.to_index_url()).unwrap();

    let response = conditional_get(
        &state.client,
        url,
        None,
        &mut entry.meta,
        state.config.settings.max_metadata_size,
    )
    .await?;

    Ok(IndexResponse {
        entry,
        status: response.status,
        data: response.data,
    })
}

/// Fetches an index entry from upstream (or stale cache) and serves it.
///
/// `window_ok` indicates the client's cached copy was filtered at the same
/// cooldown window we serve at now; only then is a `304` safe.
pub(crate) async fn forward_index(
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

            if window_ok && response.entry.meta.is_equivalent(&entry.meta) {
                index_not_modified(&response.entry, &state.config, name)
            } else {
                index_ok(&response.entry, response.data, state, name).await
            }
        }
        304 => {
            debug!("fetch: cached index entry for {name} is up to date");
            state.metadata.store(name, response.entry.clone());

            if window_ok && response.entry.meta.is_equivalent(&entry.meta) {
                index_not_modified(&response.entry, &state.config, name)
            } else if let Some(resp) = index_memo_hit(&response.entry, state, name) {
                // A memo hit needs no pristine body: skip the disk read.
                resp
            } else if let Some(data) = cache_read_index(&state.config.index_dir, &entry).await {
                index_ok(&response.entry, data, state, name).await
            } else {
                error!("cache: lost index cache file for {name}");
                state.metadata.invalidate(name);
                error_response(503)
            }
        }
        code if (500..=599).contains(&code) => {
            // Upstream trouble: a cached copy beats failing the build. 4xx
            // stays forwarded — a 404 is a real answer, not an outage.
            if let Some(data) = cache_read_index(&state.config.index_dir, &entry).await {
                warn!("proxy: upstream returned HTTP {code} for {name}; serving cached index");
                let stale = cache_find_index(&state.config.index_dir, name)
                    .await
                    .unwrap_or_else(|| IndexEntry::new(name));
                return index_ok(&stale, data, state, name).await;
            }
            warn!("fetch: upstream returned HTTP status {code} for {name}");
            json_response(code, String::from_utf8_lossy(&response.data).into_owned())
        }
        code => {
            // Forward other upstream statuses (e.g. 404) verbatim.
            warn!("fetch: upstream returned HTTP status {code} for {name}");
            json_response(code, String::from_utf8_lossy(&response.data).into_owned())
        }
    }
}
