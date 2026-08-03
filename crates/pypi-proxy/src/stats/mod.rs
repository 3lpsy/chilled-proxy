//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

#[cfg(test)]
mod tests;

use std::path::Path;
use std::time::UNIX_EPOCH;

use chilled_core::registry::{CacheStats, CachedArtifact};

use crate::valid;

/// Scans the file cache (`<cache_dir>/files/`) into sorted [`CacheStats`].
/// Best-effort: unreadable or malformed entries are skipped, not fatal.
pub(crate) fn cache_stats(files_dir: &Path) -> CacheStats {
    let mut artifacts = Vec::new();
    let Ok(project_dirs) = std::fs::read_dir(files_dir) else {
        return CacheStats::default();
    };

    for project_dir in project_dirs.flatten() {
        if !project_dir.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = project_dir.file_name().to_string_lossy().into_owned();
        if !valid::is_valid_name(&name) || valid::normalize(&name) != name {
            continue;
        }

        let Ok(files) = std::fs::read_dir(project_dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let file_name = file.file_name().to_string_lossy().into_owned();
            if !valid::is_valid_filename(&file_name) {
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
                version: file_name,
                cached_at,
            });
        }
    }
    artifacts.sort();
    CacheStats { artifacts }
}
