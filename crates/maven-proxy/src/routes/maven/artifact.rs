//! Serving artifact files: content types, the fail-closed download gate, the
//! disk cache, and the upstream fetch.

use axum::response::Response;
use bytes::Bytes;
use chilled_core::cache::fs::{fetch_file, store_file};
use chilled_core::http::{data_response, read_capped, text_response, FetchError};
use log::{debug, error, info, warn};

use crate::checksum::split_checksum;
use crate::constants::{JAR_CTYPE, OCTET_CTYPE, TEXT_CTYPE, XML_CTYPE};
use crate::coords::MavenCoords;
use crate::probe;
use crate::routes::maven::handler::plain_error;
use crate::routes::maven::metadata::sidecar_path;
use crate::sidecar::VersionTimes;
use crate::state::AppState;

pub(super) fn ctype_for(file: &str) -> &'static str {
    let (base, algo) = split_checksum(file);
    if algo.is_some() {
        return TEXT_CTYPE;
    }
    if base.ends_with(".jar") {
        JAR_CTYPE
    } else if base.ends_with(".pom") || base.ends_with(".xml") {
        XML_CTYPE
    } else {
        OCTET_CTYPE
    }
}

/// The download gate's verdict for one version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Old enough to serve.
    Allow,
    /// Inside the cooldown window, or undatable — refuse (403).
    Refuse,
    /// Upstream does not carry this version at all — not found (404).
    NotFound,
}

impl From<bool> for Gate {
    fn from(old_enough: bool) -> Self {
        if old_enough {
            Gate::Allow
        } else {
            Gate::Refuse
        }
    }
}

/// Whether this version may be downloaded under `--restrict-downloads`.
/// **Fail-closed**: the sidecar age (probed on demand) must exist and be
/// `<= cutoff` — except that upstream reporting the version absent is a
/// definite answer and becomes a 404 rather than a refusal.
async fn artifact_old_enough(
    state: &AppState,
    coords: &MavenCoords,
    version: &str,
    cutoff: u64,
) -> Gate {
    let side_path = sidecar_path(state, coords);
    let load_path = side_path.clone();
    let Ok(mut times) = tokio::task::spawn_blocking(move || VersionTimes::load(&load_path)).await
    else {
        return Gate::Refuse;
    };

    // A first-seen guess is retried while it still gates, so a transient probe
    // failure does not refuse an old artifact for a whole window.
    if let Some(ts) = times.get(version) {
        if !(times.is_provisional(version) && ts > cutoff) {
            return Gate::from(ts <= cutoff);
        }
    }

    let stamp = match probe::probe_version(
        &state.client,
        &state.config.upstream_url,
        coords,
        version,
    )
    .await
    {
        probe::Probed::Stamped(stamp) => stamp,
        // Nothing to record: the version is not in this repository, so a
        // first-seen stamp would only pollute the sidecar with a version that
        // does not exist — and gate it for a window if it ever appears.
        probe::Probed::Absent => return Gate::NotFound,
    };
    let ts = stamp.ts;
    times.insert(version.to_owned(), stamp);
    let _ = tokio::task::spawn_blocking(move || times.save(&side_path)).await;
    Gate::from(ts <= cutoff)
}

/// Downloads an artifact file from upstream; errors come back as ready-made
/// responses to forward.
async fn fetch_artifact(state: &AppState, rel: &str) -> Result<Bytes, Response> {
    let url = state
        .config
        .upstream_url
        .join(rel)
        .expect("validated segments join onto the pinned upstream URL");

    let mut response = state.client.get(url).send().await.map_err(|err| {
        error!("fetch: artifact connection failed for {rel}: {err}");
        plain_error(502, "upstream fetch failed")
    })?;

    let status = response.status();
    if !status.is_success() {
        let code = status.as_u16();
        warn!("fetch: upstream returned HTTP status {code} for {rel}");
        let body = response.text().await.unwrap_or_default();
        return Err(text_response(code, TEXT_CTYPE, body));
    }

    match read_capped(&mut response, state.config.settings.max_artifact_size).await {
        Ok(data) => Ok(Bytes::from(data)),
        Err(FetchError::TooLarge) => Err(plain_error(507, "artifact too large")),
        Err(FetchError::Http(err)) => {
            error!("fetch: artifact read failed for {rel}: {err}");
            Err(plain_error(502, "upstream fetch failed"))
        }
    }
}

/// Serves an artifact file: restrict gate, disk cache, then upstream.
pub(super) async fn serve_artifact(
    state: &AppState,
    coords: &MavenCoords,
    version: &str,
    file: &str,
) -> Response {
    // Fail-closed download gate, before any cache read.
    if state.config.settings.restrict_downloads {
        if let Some(cutoff) = state.config.cutoff_for(coords) {
            match artifact_old_enough(state, coords, version, cutoff).await {
                Gate::Allow => {}
                Gate::Refuse => {
                    warn!(
                        "download: refused {coords}:{version}: newer than cooldown or unverifiable"
                    );
                    return plain_error(403, "version is within the cooldown window");
                }
                Gate::NotFound => {
                    debug!("download: {coords}:{version} is not in this repository");
                    return plain_error(404, "not found");
                }
            }
        }
    }

    let rel = format!("{}/{version}/{file}", coords.dir_rel());
    let path = state.config.repo_dir.join(&rel);

    let read_path = path.clone();
    let cached = tokio::task::spawn_blocking(move || fetch_file(&read_path))
        .await
        .ok()
        .flatten();
    if let Some(data) = cached {
        info!("cache: served cached {rel} ({} bytes)", data.len());
        return data_response(ctype_for(file), Bytes::from(data));
    }

    match fetch_artifact(state, &rel).await {
        Ok(data) => {
            // Store off-thread; `Bytes` clones are cheap (refcounted).
            let stored = data.clone();
            let _ = tokio::task::spawn_blocking(move || store_file(&path, &stored, None)).await;
            info!("cache: stored new artifact {rel} ({} bytes)", data.len());
            data_response(ctype_for(file), data)
        }
        Err(response) => response,
    }
}
