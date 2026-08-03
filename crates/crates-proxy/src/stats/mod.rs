//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

#[cfg(test)]
mod tests;

use std::path::Path;
use std::time::UNIX_EPOCH;

use chilled_core::registry::{CacheStats, CachedArtifact};

use crate::valid;

/// Scans the crate file cache into sorted [`CacheStats`]. Best-effort:
/// unreadable or malformed entries are skipped rather than failing the report.
pub(crate) fn cache_stats(crates_dir: &Path) -> CacheStats {
    let mut artifacts = Vec::new();
    let Ok(crate_dirs) = std::fs::read_dir(crates_dir) else {
        return CacheStats::default();
    };

    for crate_dir in crate_dirs.flatten() {
        if !crate_dir.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = crate_dir.file_name().to_string_lossy().into_owned();
        if !valid::is_crate_name(&name) {
            continue;
        }

        let Ok(files) = std::fs::read_dir(crate_dir.path()) else {
            continue;
        };
        let prefix = format!("{name}-");
        for file in files.flatten() {
            let file_name = file.file_name().to_string_lossy().into_owned();
            let Some(version) = file_name
                .strip_suffix(".crate")
                .and_then(|rest| rest.strip_prefix(&prefix))
            else {
                continue;
            };
            if !valid::is_crate_version(version) {
                continue;
            }
            let cached_at = file
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            artifacts.push(CachedArtifact {
                name: name.clone(),
                version: version.to_owned(),
                cached_at,
            });
        }
    }
    artifacts.sort();
    CacheStats { artifacts }
}
