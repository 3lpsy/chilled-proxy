//! Serving distribution files: the age gate, cache, and upstream fetch.

use axum::response::Response;
use bytes::Bytes;
use chilled_core::http::{data_response, error_response, read_capped, text_response, FetchError};
use log::{debug, error, info, warn};
use serde_json::Value;

use url::Url;

use crate::constants::{FILE_CTYPE, METADATA_SUFFIX, TEXT_CTYPE};
use crate::model::PypiEntry;
use crate::routes::pypi::fetch::{download_simple, is_json_simple};
use crate::routes::pypi::serve::{cache_read_simple, cache_write_simple};
use crate::state::AppState;
use crate::{filter, valid};

/// The index entry for `filename`, from the document the proxy itself fetched.
///
/// A PEP 658 `.metadata` sidecar has no entry of its own — it is described by,
/// and ages with, its distribution — so the lookup is done under the
/// distribution's name.
async fn lookup_file(state: &AppState, project: &str, filename: &str) -> Option<Value> {
    let mut data = cache_read_simple(&state.config.simple_dir, project).await;

    // A client installing from a fully pinned lockfile may never request the
    // index, so fetch it on demand rather than refusing an old distribution.
    if data.is_none() {
        debug!("download: fetching simple index for {project} to resolve {filename}");
        if let Ok(response) = download_simple(state, PypiEntry::new(project)).await {
            if response.status == 200 && is_json_simple(&response.ctype) {
                cache_write_simple(&state.config.simple_dir, &response.entry, &response.data).await;
                state.metadata.store(project, response.entry.clone());
                data = Some(response.data);
            }
        }
    }

    let data = data?;
    let wanted = valid::distribution_name(filename).to_owned();
    tokio::task::spawn_blocking(move || -> Option<Value> {
        let doc: Value = serde_json::from_slice(&data).ok()?;
        doc.get("files")?
            .as_array()?
            .iter()
            .find(|f| f.get("filename").and_then(Value::as_str) == Some(wanted.as_str()))
            .cloned()
    })
    .await
    .ok()
    .flatten()
}

/// Whether `entry` is old enough to serve under the cooldown.
fn entry_old_enough(entry: Option<&Value>, cutoff: u64) -> bool {
    let secs = entry
        .and_then(|f| f.get("upload-time"))
        .and_then(Value::as_str)
        .and_then(filter::parse_upload_time);
    matches!(secs, Some(secs) if secs <= cutoff)
}

/// The upstream URL to fetch `filename` from.
///
/// The index names each file's host itself, which is the only way a mount can
/// serve an index whose files are spread across several (PyTorch keeps `torch`
/// on its own CDN and its dependencies on PyPI's). That host is upstream-
/// controlled, so it is honored only when the operator has allowed it; anything
/// else falls back to substituting the pinned files URL, which is both the
/// single-host case and what `--pypi-files-url` exists to do for an operator
/// mirroring PyPI's file host somewhere else.
fn upstream_file_url(
    state: &AppState,
    entry: Option<&Value>,
    fhp_path: &str,
    filename: &str,
    label: &str,
) -> Option<Url> {
    let from_doc = entry
        .and_then(|f| f.get("url"))
        .and_then(Value::as_str)
        .and_then(|raw| Url::parse(raw).ok());

    if let Some(url) = from_doc {
        if state.config.allows_file_host(&url) {
            // The PEP 658 sidecar sits beside its distribution on the same host.
            return if filename.ends_with(METADATA_SUFFIX) {
                Some(Url::parse(&format!("{url}{METADATA_SUFFIX}")).unwrap_or(url))
            } else {
                Some(url)
            };
        }
        // Substituting is right for a mirrored file host and wrong for a
        // genuinely multi-host index, and only the operator can tell them
        // apart — so say which host was skipped and how to allow it. This is
        // the misconfiguration signal for a PyTorch-style mount, so it must be
        // visible at the default log level, not buried in debug.
        warn!(
            "download: {label}: index names host '{}', not allowed for this mount; \
             falling back to the pinned files URL (allow it with `file-hosts` if this 404s)",
            url.host_str().unwrap_or("?")
        );
    }

    state.config.files_url.join(fhp_path).ok()
}

/// Downloads a distribution file from `url`.
///
/// On an upstream HTTP error or a transport failure, returns a ready-made
/// error `Response` to forward to the client.
async fn download_file(state: &AppState, url: Url, label: &str) -> Result<Bytes, Response> {
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

    // One lookup serves both the age gate and the URL: the entry that says how
    // old a file is also says where it lives, so the two can never disagree.
    // Skipped entirely when neither needs it, keeping the cached-hit path free
    // of index work.
    let gate = state
        .config
        .settings
        .restrict_downloads
        .then(|| state.config.cutoff_for(project))
        .flatten();
    let entry = if gate.is_some() {
        lookup_file(state, project, filename).await
    } else {
        None
    };

    // With --restrict-downloads, refuse files newer than the cooldown even if
    // requested directly (fail-closed, before any cache read).
    if let Some(cutoff) = gate {
        if !entry_old_enough(entry.as_ref(), cutoff) {
            warn!("download: refused {label}: newer than cooldown or unverifiable");
            return text_response(
                403,
                TEXT_CTYPE,
                "download refused by cooldown policy\n".into(),
            );
        }
    }

    // Cache under the full relative path (already segment-validated), not just
    // the filename: a multi-host index can carry same-named files at different
    // paths (`whl/cpu/…` vs `whl/cu118/…`) that must not collide.
    let file_path = state.config.files_dir.join(project).join(fhp_path);
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

    // Resolve only now: a cache hit never needs to know where upstream keeps it.
    let entry = match entry {
        Some(entry) => Some(entry),
        None => lookup_file(state, project, filename).await,
    };
    let Some(url) = upstream_file_url(state, entry.as_ref(), fhp_path, filename, &label) else {
        warn!("download: cannot build upstream file URL for {label}");
        return error_response(404);
    };

    match download_file(state, url, &label).await {
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
