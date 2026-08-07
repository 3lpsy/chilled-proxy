//! Cache deletion, reported through `RegistryProxy::{purge_artifact,purge_all}`.

use std::path::Path;

use crate::constants::METADATA_FILE;

/// Checksum/signature sidecars: deleted with the version but not re-fetched
/// directly — clients request them on demand.
const CHECKSUM_EXTS: &[&str] = &[".sha1", ".md5", ".sha256", ".sha512", ".asc"];

/// Deletes one cached version directory (`name` is `group:artifact`); returns
/// the artifact-file paths that re-fetch it.
pub(crate) fn purge_artifact(repo_dir: &Path, name: &str, version: &str) -> Vec<String> {
    let Some((group, artifact)) = name.split_once(':') else {
        return Vec::new();
    };
    let group_path = group.replace('.', "/");
    let dir = repo_dir.join(&group_path).join(artifact).join(version);
    let mut refetch = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return refetch;
    };
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if std::fs::remove_file(entry.path()).is_err() {
            continue;
        }
        let sidecar = CHECKSUM_EXTS.iter().any(|ext| file.ends_with(ext));
        if !sidecar && file != METADATA_FILE && !file.starts_with('.') {
            refetch.push(format!("/{group_path}/{artifact}/{version}/{file}"));
        }
    }
    let _ = std::fs::remove_dir(&dir);
    refetch
}

/// Deletes every cached repository file (in-memory metadata caches stay).
pub(crate) fn purge_all(repo_dir: &Path) {
    let _ = std::fs::remove_dir_all(repo_dir);
    let _ = std::fs::create_dir_all(repo_dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purges_a_version_dir_and_skips_sidecars_in_refetch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path();
        let dir = repo.join("com/example/thing/1.0.0");
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["thing-1.0.0.jar", "thing-1.0.0.jar.sha1", "thing-1.0.0.pom"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }

        let mut paths = purge_artifact(repo, "com.example:thing", "1.0.0");
        paths.sort();
        assert_eq!(
            paths,
            [
                "/com/example/thing/1.0.0/thing-1.0.0.jar",
                "/com/example/thing/1.0.0/thing-1.0.0.pom",
            ]
        );
        assert!(!dir.exists());
        assert!(purge_artifact(repo, "com.example:thing", "1.0.0").is_empty());
    }
}
