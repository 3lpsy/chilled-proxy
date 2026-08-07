//! Cache deletion, reported through `RegistryProxy::{purge_artifact,purge_all}`.

use std::path::Path;

/// Deletes one cached crate file; returns the download path that re-fetches it.
pub(crate) fn purge_artifact(crates_dir: &Path, name: &str, version: &str) -> Vec<String> {
    let path = crates_dir
        .join(name)
        .join(format!("{name}-{version}.crate"));
    if std::fs::remove_file(&path).is_ok() {
        vec![format!("/api/v1/crates/{name}/{version}/download")]
    } else {
        Vec::new()
    }
}

/// Deletes every cached crate file (the index cache stays).
pub(crate) fn purge_all(crates_dir: &Path) {
    let _ = std::fs::remove_dir_all(crates_dir);
    let _ = std::fs::create_dir_all(crates_dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purges_one_version_and_reports_its_download_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("serde")).unwrap();
        std::fs::write(dir.join("serde/serde-1.0.0.crate"), b"x").unwrap();
        std::fs::write(dir.join("serde/serde-2.0.0.crate"), b"x").unwrap();

        let paths = purge_artifact(dir, "serde", "1.0.0");
        assert_eq!(paths, ["/api/v1/crates/serde/1.0.0/download"]);
        assert!(!dir.join("serde/serde-1.0.0.crate").exists());
        assert!(dir.join("serde/serde-2.0.0.crate").exists());
        assert!(purge_artifact(dir, "serde", "1.0.0").is_empty());

        purge_all(dir);
        assert!(!dir.join("serde/serde-2.0.0.crate").exists());
        assert!(dir.exists());
    }
}
