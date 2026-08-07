//! `GET /api/v1/crates/<path>` — proxied, cached crate file downloads.

use std::path::Path;

use axum::{
    extract::{Path as UrlPath, State},
    response::Response,
};
use bytes::Bytes;
use chilled_core::http::{data_response, error_response, json_response, read_capped, FetchError};
use log::{debug, error, info, warn};

use crate::cache::{
    cache_fetch_crate, cache_fetch_index_entry, cache_store_crate, CrateInfo, IndexEntry,
};
use crate::constants::{CRATES_API_PATH, CRATE_CTYPE};
use crate::filter;
use crate::http::format_json_error;
use crate::routes::index::cache_write_index;
use crate::state::AppState;

/// Reads a cached crate file off the blocking thread pool.
async fn cache_read_crate(dir: &Path, info: &CrateInfo) -> Option<Vec<u8>> {
    let dir = dir.to_path_buf();
    let info = info.clone();
    tokio::task::spawn_blocking(move || cache_fetch_crate(&dir, &info))
        .await
        .ok()
        .flatten()
}

/// Whether this version may be downloaded under `--restrict-downloads`, per
/// the locally cached *pristine* index entry's `pubtime`. **Fail-closed**: no
/// cached index, unknown version, or a too-new `pubtime` all refuse.
pub(crate) async fn download_old_enough(state: &AppState, info: &CrateInfo, cutoff: u64) -> bool {
    let dir = state.config.index_dir.clone();
    let entry = IndexEntry::new(info.name());
    let mut body = tokio::task::spawn_blocking(move || cache_fetch_index_entry(&dir, &entry))
        .await
        .ok()
        .flatten();

    // A build resolving straight from a lockfile may never request the index,
    // so fetch it on demand rather than refusing an otherwise-old version.
    if body.is_none() {
        debug!(
            "download: fetching index entry for {} to age-check {info}",
            info.name()
        );
        body = fetch_index_for_gate(state, info.name()).await;
    }

    let Some(body) = body else { return false };
    let Ok(text) = std::str::from_utf8(&body) else {
        return false;
    };
    matches!(filter::version_pubtime(text, info.version()), Some(pt) if pt <= cutoff)
}

/// Fetches the pristine index entry for `name` so the age gate has something
/// to check, caching it on the way through. `None` leaves the gate closed.
async fn fetch_index_for_gate(state: &AppState, name: &str) -> Option<Vec<u8>> {
    let response = super::index::download_index_entry(state, IndexEntry::new(name))
        .await
        .ok()?;
    if response.status != 200 {
        return None;
    }
    cache_write_index(&state.config.index_dir, &response.entry, &response.data).await;
    state.metadata.store(name, response.entry.clone());
    Some(response.data)
}

/// Downloads a crate file from the upstream download server.
///
/// On an upstream HTTP error or a transport failure, returns a ready-made
/// error `Response` to forward to the client.
async fn download_crate(state: &AppState, info: &CrateInfo) -> Result<Bytes, Response> {
    let url = state
        .config
        .upstream_url
        .join(CRATES_API_PATH)
        .unwrap()
        .join(&info.to_download_url())
        .unwrap();

    let mut response = state.client.get(url).send().await.map_err(|e| {
        error!("fetch: crate connection failed for {info}: {e}");
        json_response(502, format_json_error(e))
    })?;

    let status = response.status();
    if !status.is_success() {
        let code = status.as_u16();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| format_json_error("upstream error"));
        warn!("fetch: upstream returned HTTP status {code} for {info}");
        return Err(json_response(code, body));
    }

    match read_capped(&mut response, state.config.settings.max_artifact_size).await {
        Ok(data) => Ok(Bytes::from(data)),
        Err(FetchError::TooLarge) => Err(error_response(507)),
        Err(FetchError::Http(e)) => {
            error!("fetch: crate read failed for {info}: {e}");
            Err(json_response(502, format_json_error(e)))
        }
    }
}

/// Handles a crate download request: `GET /api/v1/crates/<path>`.
pub(crate) async fn handle_download(
    State(state): State<AppState>,
    UrlPath(path): UrlPath<String>,
) -> Response {
    let Some(crate_info) = CrateInfo::try_from_download_url(&path) else {
        warn!("proxy: unrecognized download API endpoint: {path}");
        return error_response(404);
    };

    // With --restrict-downloads, refuse versions newer than the cooldown even
    // if requested directly (e.g. a hand-edited Cargo.lock).
    if state.config.settings.restrict_downloads {
        if let Some(cutoff) = state.config.cutoff_for(crate_info.name()) {
            if !download_old_enough(&state, &crate_info, cutoff).await {
                warn!("download: refused {crate_info}: newer than cooldown or unverifiable");
                return json_response(
                    403,
                    format_json_error("cooldown: version not old enough or unverifiable"),
                );
            }
        }
    }

    if let Some(data) = cache_read_crate(&state.config.crates_dir, &crate_info).await {
        info!(
            "cache: served cached crate {crate_info} ({} bytes)",
            data.len()
        );
        return data_response(CRATE_CTYPE, Bytes::from(data));
    }

    match download_crate(&state, &crate_info).await {
        Ok(data) => {
            // Store off-thread; `Bytes` clones are cheap (refcounted).
            let dir = state.config.crates_dir.clone();
            let info = crate_info.clone();
            let stored = data.clone();
            let _ =
                tokio::task::spawn_blocking(move || cache_store_crate(&dir, &info, &stored)).await;
            info!(
                "cache: stored new crate {crate_info} ({} bytes)",
                data.len()
            );
            data_response(CRATE_CTYPE, data)
        }
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    //! The fail-closed gate and proxy flow are exercised end-to-end by the
    //! `downloads` integration suite; unit coverage here sticks to URL parsing glue.

    use crate::cache::CrateInfo;

    #[test]
    fn download_path_round_trips_through_crate_info() {
        let info = CrateInfo::try_from_download_url("serde/1.0.0/download").unwrap();
        assert_eq!(info.to_download_url(), "serde/1.0.0/download");
    }
}
