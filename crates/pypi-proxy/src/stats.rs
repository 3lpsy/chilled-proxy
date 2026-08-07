//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

use std::path::Path;
use std::time::UNIX_EPOCH;

use chilled_core::registry::{CacheStats, CachedArtifact};

use crate::constants::METADATA_SUFFIX;
use crate::valid;

/// How deep the walk below a project directory may go — matches the segment
/// cap on the files route, plus slack.
const MAX_SCAN_DEPTH: usize = 10;

/// The release version a cached filename belongs to. PEP 658 `.metadata`
/// sidecars count toward their distribution; unparseable names fall back to
/// the filename so nothing disappears from the report.
pub(crate) fn file_version(filename: &str) -> String {
    let base = filename.strip_suffix(METADATA_SUFFIX).unwrap_or(filename);
    parse_version(base).unwrap_or_else(|| filename.to_owned())
}

/// `{dist}-{version}-…​.whl` or `{dist}-{version}.tar.gz`-style parsing.
fn parse_version(base: &str) -> Option<String> {
    if let Some(stem) = base.strip_suffix(".whl") {
        // Wheel: distribution-version(-build)-python-abi-platform.
        return stem.split('-').nth(1).map(str::to_owned);
    }
    let stem = base
        .strip_suffix(".tar.gz")
        .or_else(|| base.strip_suffix(".tar.bz2"))
        .or_else(|| base.strip_suffix(".zip"))?;
    // Sdist: version is the last dash segment and starts with a digit.
    let (_, version) = stem.rsplit_once('-')?;
    version
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        .then(|| version.to_owned())
}

/// Scans the file cache (`<cache_dir>/files/`) into sorted [`CacheStats`].
/// Files are cached under their full upstream-relative path, so the walk
/// recurses below each project directory; the several files of one release
/// (wheels, sdist, metadata sidecars) report as one artifact, sizes summed.
pub(crate) fn cache_stats(files_dir: &Path) -> CacheStats {
    let mut artifacts = Vec::new();
    // A missing directory is an empty cache; any other error (fd exhaustion,
    // I/O) must not read as empty or consumers would wrongly prune.
    let project_dirs = match std::fs::read_dir(files_dir) {
        Ok(dirs) => dirs,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CacheStats::default(),
        Err(_) => {
            return CacheStats {
                incomplete: true,
                ..Default::default()
            }
        }
    };

    for project_dir in project_dirs.flatten() {
        if !project_dir.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = project_dir.file_name().to_string_lossy().into_owned();
        if !valid::is_valid_name(&name) || valid::normalize(&name) != name {
            continue;
        }
        walk(&project_dir.path(), &name, MAX_SCAN_DEPTH, &mut artifacts);
    }
    artifacts.sort();
    artifacts.dedup_by(|dup, kept| {
        let same = dup.name == kept.name && dup.version == kept.version;
        if same {
            kept.cached_at = kept.cached_at.max(dup.cached_at);
            kept.size_bytes += dup.size_bytes;
        }
        same
    });
    CacheStats {
        artifacts,
        incomplete: false,
    }
}

/// Collects valid distribution files below `dir` into `artifacts`.
fn walk(dir: &Path, project: &str, depth: usize, artifacts: &mut Vec<CachedArtifact>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if depth > 0 {
                walk(&entry.path(), project, depth - 1, artifacts);
            }
            continue;
        }
        if !valid::is_valid_filename(&file_name) {
            continue;
        }
        let meta = entry.metadata().ok();
        let cached_at = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        artifacts.push(CachedArtifact {
            name: project.to_owned(),
            version: file_version(&file_name),
            cached_at,
            size_bytes: meta.map_or(0, |m| m.len()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_from_wheels_sdists_and_sidecars() {
        assert_eq!(file_version("blinker-1.9.0-py3-none-any.whl"), "1.9.0");
        assert_eq!(
            file_version("blinker-1.9.0-py3-none-any.whl.metadata"),
            "1.9.0"
        );
        assert_eq!(
            file_version("charset_normalizer-3.4.9-cp312-cp312-musllinux_1_2_x86_64.whl"),
            "3.4.9"
        );
        assert_eq!(file_version("requests-2.0.0.tar.gz"), "2.0.0");
        assert_eq!(file_version("pyyaml-6.0.2rc1.tar.gz"), "6.0.2rc1");
        // Unparseable names fall back to the filename.
        assert_eq!(file_version("garbage.whl.unknown"), "garbage.whl.unknown");
    }

    #[test]
    fn one_release_counts_once_across_its_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();

        // Wheel + sdist + metadata sidecar of one release, at different paths.
        std::fs::create_dir_all(dir.join("requests/packages/aa/bb")).unwrap();
        std::fs::write(
            dir.join("requests/packages/aa/bb/requests-2.0.0.tar.gz"),
            b"xx",
        )
        .unwrap();
        std::fs::write(dir.join("requests/requests-2.0.0-py3-none-any.whl"), b"xxx").unwrap();
        std::fs::write(
            dir.join("requests/requests-2.0.0-py3-none-any.whl.metadata"),
            b"x",
        )
        .unwrap();
        // A second release stays separate.
        std::fs::write(dir.join("requests/requests-2.1.0-py3-none-any.whl"), b"x").unwrap();
        std::fs::write(dir.join("requests/garbage.txt"), b"x").unwrap();
        // Non-normalized project dir is skipped entirely.
        std::fs::create_dir_all(dir.join("Bad_Name")).unwrap();
        std::fs::write(dir.join("Bad_Name/bad-1.0.0.whl"), b"x").unwrap();

        let stats = cache_stats(dir);
        let got: Vec<_> = stats
            .artifacts
            .iter()
            .map(|a| format!("{}/{} ({}B)", a.name, a.version, a.size_bytes))
            .collect();
        assert_eq!(got, ["requests/2.0.0 (6B)", "requests/2.1.0 (1B)"]);
        assert!(stats.artifacts.iter().all(|a| a.cached_at > 0));
    }

    #[test]
    fn missing_dir_yields_empty_stats() {
        let stats = cache_stats(Path::new("/definitely/not/here"));
        assert!(stats.artifacts.is_empty());
    }
}
