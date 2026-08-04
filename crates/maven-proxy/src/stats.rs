//! Cache statistics scan, reported through `RegistryProxy::cache_stats`.

use std::path::Path;
use std::time::UNIX_EPOCH;

use chilled_core::registry::{CacheStats, CachedArtifact};

use crate::constants::METADATA_FILE;

/// Maximum directory depth walked below the repo root.
const MAX_DEPTH: usize = 32;

/// Sidecar suffixes that describe an artifact already counted.
const CHECKSUM_EXTS: &[&str] = &[".sha1", ".md5", ".sha256", ".sha512", ".asc"];

/// Scans the repository cache into sorted [`CacheStats`]. Best-effort:
/// unreadable or malformed entries are skipped rather than failing the report.
pub(crate) fn cache_stats(repo_dir: &Path) -> CacheStats {
    let mut artifacts = Vec::new();
    let mut segs = Vec::new();
    walk(repo_dir, &mut segs, &mut artifacts);
    artifacts.sort();
    // One cached version yields several files (jar, pom, ...); report it once,
    // stamped with the newest of them, so counts mean the same thing as in the
    // other registries.
    artifacts.dedup_by(|dup, kept| {
        let same = dup.name == kept.name && dup.version == kept.version;
        if same {
            kept.cached_at = kept.cached_at.max(dup.cached_at);
        }
        same
    });
    CacheStats { artifacts }
}

/// Recursively collects artifact files at `{group...}/{artifact}/{version}/{file}`,
/// skipping sidecars, metadata files, and hidden entries.
fn walk(dir: &Path, segs: &mut Vec<String>, out: &mut Vec<CachedArtifact>) {
    if segs.len() > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            segs.push(name);
            walk(&entry.path(), segs, out);
            segs.pop();
            continue;
        }
        // Need at least group/artifact/version dirs above a counted file, and
        // checksum/signature sidecars describe a file already counted.
        let sidecar = CHECKSUM_EXTS.iter().any(|ext| name.ends_with(ext));
        if !file_type.is_file() || name == METADATA_FILE || sidecar || segs.len() < 3 {
            continue;
        }
        let version = segs[segs.len() - 1].clone();
        let artifact = &segs[segs.len() - 2];
        let group = segs[..segs.len() - 2].join(".");
        let cached_at = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        out.push(CachedArtifact {
            name: format!("{group}:{artifact}"),
            version,
            cached_at,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::cache_stats;

    #[test]
    fn scans_artifact_files_and_skips_metadata_and_sidecars() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();

        let dir = repo.join("org/apache/commons/commons-lang3/3.14.0");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("commons-lang3-3.14.0.jar"), b"jar").unwrap();
        fs::write(dir.join("commons-lang3-3.14.0.pom"), b"pom").unwrap();

        let art_dir = repo.join("org/apache/commons/commons-lang3");
        fs::write(art_dir.join("maven-metadata.xml"), b"<metadata/>").unwrap();
        fs::write(art_dir.join(".chilled-versions.json"), b"{}").unwrap();

        // Too-shallow file (no version dir): skipped.
        fs::write(repo.join("org/apache/stray.jar"), b"x").unwrap();

        // The jar and pom describe one cached version, reported once.
        let stats = cache_stats(repo);
        assert_eq!(stats.artifacts.len(), 1);
        let artifact = &stats.artifacts[0];
        assert_eq!(artifact.name, "org.apache.commons:commons-lang3");
        assert_eq!(artifact.version, "3.14.0");
        assert!(artifact.cached_at > 0);
    }

    #[test]
    fn missing_repo_dir_yields_empty_stats() {
        let tmp = tempfile::tempdir().unwrap();
        let stats = cache_stats(&tmp.path().join("nope"));
        assert!(stats.artifacts.is_empty());
    }

    #[test]
    fn results_are_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        for (g, a, v) in [("zed", "z", "2.0"), ("abc", "a", "1.0")] {
            let dir = repo.join(g).join(a).join(v);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("{a}-{v}.jar")), b"j").unwrap();
        }
        let stats = cache_stats(repo);
        assert_eq!(stats.artifacts[0].name, "abc:a");
        assert_eq!(stats.artifacts[1].name, "zed:z");
    }

    #[test]
    fn one_version_counts_once_across_its_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("com/example/thing/1.0.0");
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "thing-1.0.0.jar",
            "thing-1.0.0.jar.sha1",
            "thing-1.0.0.pom",
            "thing-1.0.0.pom.md5",
            "thing-1.0.0.jar.asc",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }

        let stats = cache_stats(tmp.path());
        assert_eq!(stats.artifacts.len(), 1);
        assert_eq!(stats.artifacts[0].name, "com.example:thing");
        assert_eq!(stats.artifacts[0].version, "1.0.0");
    }
}
