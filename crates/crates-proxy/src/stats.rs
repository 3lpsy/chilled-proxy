//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_only_wellformed_cached_crates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();

        // Two valid crates, one malformed filename, one invalid crate-name dir.
        std::fs::create_dir_all(dir.join("serde")).unwrap();
        std::fs::write(dir.join("serde/serde-1.0.0.crate"), b"x").unwrap();
        std::fs::write(dir.join("serde/serde-2.0.0.crate"), b"x").unwrap();
        std::fs::write(dir.join("serde/garbage.txt"), b"x").unwrap();
        std::fs::create_dir_all(dir.join("bad name!")).unwrap();
        std::fs::write(dir.join("bad name!/bad name!-1.0.0.crate"), b"x").unwrap();

        let stats = cache_stats(dir);
        let got: Vec<_> = stats
            .artifacts
            .iter()
            .map(|a| format!("{}-{}", a.name, a.version))
            .collect();
        assert_eq!(got, ["serde-1.0.0", "serde-2.0.0"]);
        assert!(stats.artifacts.iter().all(|a| a.cached_at > 0));
    }

    #[test]
    fn missing_dir_yields_empty_stats() {
        let stats = cache_stats(Path::new("/definitely/not/here"));
        assert!(stats.artifacts.is_empty());
    }
}
