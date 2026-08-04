//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_only_wellformed_cached_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();

        std::fs::create_dir_all(dir.join("requests")).unwrap();
        std::fs::write(dir.join("requests/requests-2.0.0.tar.gz"), b"x").unwrap();
        std::fs::write(dir.join("requests/requests-2.0.0-py3-none-any.whl"), b"x").unwrap();
        std::fs::write(dir.join("requests/garbage.txt"), b"x").unwrap();
        // Non-normalized project dir is skipped entirely.
        std::fs::create_dir_all(dir.join("Bad_Name")).unwrap();
        std::fs::write(dir.join("Bad_Name/bad-1.0.0.whl"), b"x").unwrap();

        let stats = cache_stats(dir);
        let got: Vec<_> = stats
            .artifacts
            .iter()
            .map(|a| format!("{}/{}", a.name, a.version))
            .collect();
        assert_eq!(
            got,
            [
                "requests/requests-2.0.0-py3-none-any.whl",
                "requests/requests-2.0.0.tar.gz",
            ]
        );
        assert!(stats.artifacts.iter().all(|a| a.cached_at > 0));
    }

    #[test]
    fn missing_dir_yields_empty_stats() {
        let stats = cache_stats(Path::new("/definitely/not/here"));
        assert!(stats.artifacts.is_empty());
    }
}
