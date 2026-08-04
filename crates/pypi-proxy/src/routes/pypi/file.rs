//! Serving distribution files: the age gate, cache, and upstream fetch.

use axum::response::Response;
use bytes::Bytes;
use chilled_core::http::{data_response, error_response, read_capped, text_response, FetchError};
use log::{debug, error, info, warn};
use serde_json::Value;

use crate::constants::{FILE_CTYPE, TEXT_CTYPE};
use crate::model::{cache_store_simple, PypiEntry};
use crate::routes::pypi::fetch::{download_simple, is_json_simple};
use crate::routes::pypi::serve::cache_read_simple;
use crate::state::AppState;
use crate::{filter, valid};

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

    match read_capped(&mut response, state.config.settings.max_artifact_size).await {
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
pub(super) async fn serve_file(
    state: &AppState,
    project: &str,
    fhp_path: &str,
    filename: &str,
) -> Response {
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
