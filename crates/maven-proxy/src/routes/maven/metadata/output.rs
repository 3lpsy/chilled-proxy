//! Producing the filtered (or pristine) metadata body and serving it as XML
//! or as a generated checksum.

use axum::{body::Body, http::header, response::Response};
use bytes::Bytes;
use chilled_core::cache::MEMO_BUCKET_SECS;
use chilled_core::etag::{cooldown_validators, Marker};
use chilled_core::http::text_response;
use log::{error, info};

use crate::checksum::ChecksumAlgo;
use crate::constants::{TEXT_CTYPE, XML_CTYPE};
use crate::coords::MavenCoords;
use crate::filter;
use crate::model::MavenEntry;
use crate::probe;
use crate::routes::maven::handler::plain_error;
use crate::routes::maven::metadata::serve::sidecar_path;
use crate::sidecar::VersionTimes;
use crate::state::AppState;

/// Produces the filtered (or pristine) metadata body and serves it as XML or
/// as a generated checksum. The single source of truth for both routes keeps
/// `maven-metadata.xml.{algo}` coherent with the served `.xml` bytes.
pub(super) async fn metadata_ok(
    state: &AppState,
    coords: &MavenCoords,
    entry: &MavenEntry,
    data: Vec<u8>,
    algo: Option<ChecksumAlgo>,
) -> Response {
    let Some(cutoff) = state.config.cutoff_for(coords) else {
        // Unfiltered: serve verbatim with the upstream validators.
        return match algo {
            None => cooldown_validators(
                Response::builder()
                    .status(200)
                    .header(header::CONTENT_TYPE, XML_CTYPE),
                entry,
                None,
            )
            .body(Body::from(data))
            .expect("valid metadata response"),
            // Rare race: cooldown vanished after routing; hash the pristine body.
            Some(algo) => text_response(200, TEXT_CTYPE, algo.hex(&data)),
        };
    };

    let bucket = cutoff / MEMO_BUCKET_SECS;
    let validator = entry.validator();
    let key = coords.dir_rel();

    // The sidecar also shapes the output, but only ever monotonically (versions
    // aging in); the hour-granular bucket accepts that ≤1h staleness.
    let body = if let Some(cached) = state.memo.get(&key, &validator, bucket) {
        cached
    } else {
        match filter_pipeline(state, coords, data, cutoff).await {
            Ok(Some(filtered)) => {
                let filtered = Bytes::from(filtered);
                state.memo.put(key, validator, bucket, filtered.clone());
                filtered
            }
            Ok(None) => {
                info!("cooldown: all versions of {coords} are within the cooldown window");
                return plain_error(404, "no versions outside the cooldown window");
            }
            Err(response) => return response,
        }
    };

    filtered_response(state, coords, entry, body, algo, bucket).await
}

/// Serves an already-produced filtered body as XML or a generated checksum.
async fn filtered_response(
    state: &AppState,
    coords: &MavenCoords,
    entry: &MavenEntry,
    body: Bytes,
    algo: Option<ChecksumAlgo>,
    bucket: u64,
) -> Response {
    match algo {
        None => cooldown_validators(
            Response::builder()
                .status(200)
                .header(header::CONTENT_TYPE, XML_CTYPE),
            entry,
            Some(Marker {
                window: state.config.settings.cooldown.as_secs(),
                bucket,
            }),
        )
        .body(Body::from(body))
        .expect("valid metadata response"),
        Some(algo) => {
            // Hash off the async workers; bodies can reach the metadata cap.
            match tokio::task::spawn_blocking(move || algo.hex(&body)).await {
                Ok(hex) => text_response(200, TEXT_CTYPE, hex),
                Err(err) => {
                    error!("cooldown: checksum task failed for {coords}: {err}");
                    plain_error(500, "internal error")
                }
            }
        }
    }
}

/// Serves the memoized filtered body for `entry` without touching the disk
/// cache — `None` on a memo miss, or when the artifact is unfiltered (the
/// verbatim pristine body is needed then).
pub(super) async fn metadata_memo_hit(
    state: &AppState,
    coords: &MavenCoords,
    entry: &MavenEntry,
    algo: Option<ChecksumAlgo>,
) -> Option<Response> {
    let cutoff = state.config.cutoff_for(coords)?;
    let bucket = cutoff / MEMO_BUCKET_SECS;
    let body = state
        .memo
        .get(&coords.dir_rel(), &entry.validator(), bucket)?;
    Some(filtered_response(state, coords, entry, body, algo, bucket).await)
}

/// Parses versions, tops up the sidecar via POM probes, persists it, and
/// filters the metadata. `Ok(None)` means no version survived.
async fn filter_pipeline(
    state: &AppState,
    coords: &MavenCoords,
    data: Vec<u8>,
    cutoff: u64,
) -> Result<Option<Vec<u8>>, Response> {
    let side_path = sidecar_path(state, coords);

    // Parse the version list and load the sidecar off-thread. The body moves
    // through the task and back rather than being cloned for it.
    let load_path = side_path.clone();
    let parsed = tokio::task::spawn_blocking(move || {
        let versions = filter::list_versions(&data)?;
        Ok::<_, String>((data, versions, VersionTimes::load(&load_path)))
    })
    .await;
    let (data, versions, mut times) = match parsed {
        Ok(Ok(parsed)) => parsed,
        Ok(Err(err)) => {
            error!("cooldown: unparseable upstream metadata for {coords}: {err}");
            return Err(plain_error(502, "unparseable upstream metadata"));
        }
        Err(err) => {
            error!("cooldown: metadata parse task failed for {coords}: {err}");
            return Err(plain_error(500, "internal error"));
        }
    };

    let changed = probe::probe_versions(
        &state.client,
        &state.config.upstream_url,
        coords,
        &versions,
        &mut times,
        cutoff,
    )
    .await;

    // Persist the sidecar and filter off-thread. A panic must not become an
    // empty 200.
    let filter_coords = coords.to_string();
    let filtered = tokio::task::spawn_blocking(move || {
        if changed {
            times.save(&side_path);
        }
        filter::filter_metadata(&data, &versions, &times, cutoff)
    })
    .await;
    match filtered {
        Ok(Ok(filtered)) => Ok(filtered),
        Ok(Err(err)) => {
            error!("cooldown: metadata filter failed for {filter_coords}: {err}");
            Err(plain_error(502, "unparseable upstream metadata"))
        }
        Err(err) => {
            error!("cooldown: metadata filter task failed for {filter_coords}: {err}");
            Err(plain_error(500, "internal error"))
        }
    }
}
