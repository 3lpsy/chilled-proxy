//! Tarball downloads: the fail-closed age gate, cache, and upstream fetch.

use axum::response::Response;
use bytes::Bytes;
use chilled_core::cache::fs::store_file;
use chilled_core::http::{data_response, error_response, json_response, read_capped, FetchError};
use chilled_core::time::parse_rfc3339z;
use log::{debug, error, info, warn};

use crate::constants::TARBALL_CTYPE;
use crate::http::format_npm_error;
use crate::model::NpmEntry;
use crate::model::PackageRef;
use crate::routes::npm::cache::{cache_read_packument, cache_read_tarball, cache_write_packument};
use crate::routes::npm::packument::download_packument;
use crate::state::AppState;

pub(super) async fn handle_tarball(
    state: &AppState,
    pkg: &PackageRef,
    file: &str,
    version: &str,
) -> Response {
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

    match read_capped(&mut response, state.config.settings.max_artifact_size).await {
        Ok(data) => Ok(Bytes::from(data)),
        Err(FetchError::TooLarge) => Err(error_response(507)),
        Err(FetchError::Http(e)) => {
            error!("fetch: tarball read failed for {pkg}: {e}");
            Err(json_response(502, format_npm_error(e)))
        }
    }
}

// --- Disk cache plumbing (blocking FS off the async workers) ---
