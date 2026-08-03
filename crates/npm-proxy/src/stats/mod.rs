//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

#[cfg(test)]
mod tests;

use std::path::Path;
use std::time::UNIX_EPOCH;

use chilled_core::registry::{CacheStats, CachedArtifact};

use crate::valid;

/// Scans the tarball cache into sorted [`CacheStats`]. Best-effort:
/// unreadable or malformed entries are skipped rather than failing the report.
pub(crate) fn cache_stats(tarballs_dir: &Path) -> CacheStats {
    let mut artifacts = Vec::new();
    let Ok(top_dirs) = std::fs::read_dir(tarballs_dir) else {
        return CacheStats::default();
    };

    for top in top_dirs.flatten() {
        if !top.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir_name = top.file_name().to_string_lossy().into_owned();
        if let Some(scope) = dir_name.strip_prefix('@') {
            // Scoped: a scope dir containing one dir per package name.
            if !valid::is_name_part(scope) {
                continue;
            }
            let Ok(name_dirs) = std::fs::read_dir(top.path()) else {
                continue;
            };
            for name_dir in name_dirs.flatten() {
                if !name_dir.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let name = name_dir.file_name().to_string_lossy().into_owned();
                if !valid::is_name_part(&name) {
                    continue;
                }
                let full_name = format!("{dir_name}/{name}");
                scan_tarballs(&name_dir.path(), &full_name, &name, &mut artifacts);
            }
        } else {
            if !valid::is_name_part(&dir_name) {
                continue;
            }
            scan_tarballs(&top.path(), &dir_name, &dir_name, &mut artifacts);
        }
    }
    artifacts.sort();
    CacheStats { artifacts }
}

/// Collects `{unscoped}-{version}.tgz` files from one package directory.
fn scan_tarballs(dir: &Path, full_name: &str, unscoped: &str, artifacts: &mut Vec<CachedArtifact>) {
    let Ok(files) = std::fs::read_dir(dir) else {
        return;
    };
    for file in files.flatten() {
        let file_name = file.file_name().to_string_lossy().into_owned();
        let Some(version) = valid::tarball_version(unscoped, &file_name) else {
            continue;
        };
        let cached_at = file
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        artifacts.push(CachedArtifact {
            name: full_name.to_owned(),
            version,
            cached_at,
        });
    }
}
