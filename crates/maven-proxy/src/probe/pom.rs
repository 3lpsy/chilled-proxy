//! POM probing and stamp bookkeeping.

use std::time::{SystemTime, UNIX_EPOCH};

use tokio::task::JoinSet;

use chilled_core::http::parse_http_date;
use log::{debug, warn};
use reqwest::header::LAST_MODIFIED;
use url::Url;

use crate::coords::MavenCoords;
use crate::sidecar::{Stamp, VersionTimes, FIRST_SEEN_SRC, LAST_MODIFIED_SRC};
use crate::valid::is_version;

/// How many POM probes run at once. Central tolerates this comfortably while
/// keeping a many-versioned artifact's first request to seconds, not minutes.
const PROBE_CONCURRENCY: usize = 12;

/// What a POM probe learned about one version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probed {
    /// Upstream answered; `stamp` is its publish time, or a fail-closed
    /// first-seen guess when the response carried no usable `Last-Modified`.
    Stamped(Stamp),
    /// Upstream said the POM is not there (404/410). That is an answer, not a
    /// failure: this repository does not carry the version at all.
    Absent,
}

impl Probed {
    /// The stamp to record, treating an absent version as fail-closed.
    pub(crate) fn stamp(self) -> Stamp {
        match self {
            Probed::Stamped(stamp) => stamp,
            Probed::Absent => first_seen_now(),
        }
    }
}

/// Probes the POM of one version (never fails). The version comes from
/// upstream XML, so it is validated here: an absolute URL or traversal in
/// `<version>` would otherwise redirect the probe off the pinned host.
pub(crate) async fn probe_version(
    client: &reqwest::Client,
    upstream: &Url,
    coords: &MavenCoords,
    version: &str,
) -> Probed {
    if !is_version(version) {
        warn!("cooldown: refusing to probe {coords} with malformed version {version:?}");
        return Probed::Stamped(first_seen_now());
    }
    let url = upstream
        .join(&coords.pom_rel(version))
        .expect("validated segments join onto the pinned upstream URL");

    let header = match client.head(url).send().await {
        Ok(resp) if resp.status().is_success() => resp
            .headers()
            .get(LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned),
        // A mount serves one repository, and a build routinely asks each of its
        // repositories for artifacts only another one carries. Absent is not
        // "unverifiable" — reporting it as gated would be a lie.
        Ok(resp) if matches!(resp.status().as_u16(), 404 | 410) => {
            debug!(
                "cooldown: pom probe for {coords}:{version} got HTTP {} — not in this repository",
                resp.status().as_u16()
            );
            return Probed::Absent;
        }
        Ok(resp) => {
            warn!(
                "cooldown: pom probe for {coords}:{version} got HTTP {}",
                resp.status().as_u16()
            );
            None
        }
        Err(err) => {
            warn!("cooldown: pom probe failed for {coords}:{version}: {err}");
            None
        }
    };
    Probed::Stamped(stamp_from_last_modified(header.as_deref()))
}

/// Ensures `times` has a usable stamp for every version, probing the missing
/// ones with bounded concurrency (thousands probed serially would read as a
/// hang). A first-seen guess from an earlier failed probe is retried while it
/// still gates. Returns `true` when anything changed.
pub(crate) async fn probe_versions(
    client: &reqwest::Client,
    upstream: &Url,
    coords: &MavenCoords,
    versions: &[String],
    times: &mut VersionTimes,
    cutoff: u64,
) -> bool {
    let pending: Vec<&String> = versions
        .iter()
        .filter(|v| !times.contains(v) || needs_retry(times, v, cutoff))
        .collect();
    if pending.is_empty() {
        return false;
    }
    debug!("cooldown: probing {} version(s) of {coords}", pending.len());

    for chunk in pending.chunks(PROBE_CONCURRENCY) {
        let mut set = JoinSet::new();
        for version in chunk {
            let (client, upstream, coords, version) = (
                client.clone(),
                upstream.clone(),
                coords.clone(),
                (*version).clone(),
            );
            set.spawn(async move {
                // Metadata filtering keeps the fail-closed reading: a version
                // the metadata lists but whose POM is missing cannot be dated,
                // so it stays hidden rather than being served undated.
                let stamp = probe_version(&client, &upstream, &coords, &version)
                    .await
                    .stamp();
                (version, stamp)
            });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((version, stamp)) => times.insert(version, stamp),
                // A panicked probe must not silently become an "old" verdict.
                Err(err) => warn!("cooldown: probe task failed for {coords}: {err}"),
            }
        }
    }
    true
}

/// Whether a recorded stamp is a first-seen guess that still gates the version
/// (once it ages past the cutoff the guess no longer changes the outcome).
pub(super) fn needs_retry(times: &VersionTimes, version: &str, cutoff: u64) -> bool {
    times.is_provisional(version) && times.get(version).is_some_and(|ts| ts > cutoff)
}

/// A first-seen stamp of now — the fail-closed fallback.
fn first_seen_now() -> Stamp {
    Stamp {
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        src: FIRST_SEEN_SRC.to_owned(),
    }
}

/// Converts an optional `Last-Modified` header into a stamp: parseable dates
/// become `"lm"` stamps; anything else falls back to first-seen `"fs"` now.
pub(crate) fn stamp_from_last_modified(header: Option<&str>) -> Stamp {
    if let Some(ts) = header
        .and_then(parse_http_date)
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    {
        return Stamp {
            ts: ts.as_secs(),
            src: LAST_MODIFIED_SRC.to_owned(),
        };
    }
    first_seen_now()
}
