//! Cache deletion, reported through `RegistryProxy::{purge_artifact,purge_all}`.

use std::path::Path;

/// Deletes one cached tarball; returns the tarball path that re-fetches it.
/// `name` may be scoped (`@scope/pkg`); the file uses the unscoped part.
pub(crate) fn purge_artifact(tarballs_dir: &Path, name: &str, version: &str) -> Vec<String> {
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    let file = format!("{unscoped}-{version}.tgz");
    let path = tarballs_dir.join(name).join(&file);
    if std::fs::remove_file(&path).is_ok() {
        vec![format!("/{name}/-/{file}")]
    } else {
        Vec::new()
    }
}

/// Deletes every cached tarball (packument caches stay).
pub(crate) fn purge_all(tarballs_dir: &Path) {
    let _ = std::fs::remove_dir_all(tarballs_dir);
    let _ = std::fs::create_dir_all(tarballs_dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purges_scoped_and_unscoped_tarballs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("lodash")).unwrap();
        std::fs::write(dir.join("lodash/lodash-4.17.21.tgz"), b"x").unwrap();
        std::fs::create_dir_all(dir.join("@scope/pkg")).unwrap();
        std::fs::write(dir.join("@scope/pkg/pkg-1.0.0.tgz"), b"x").unwrap();

        assert_eq!(
            purge_artifact(dir, "lodash", "4.17.21"),
            ["/lodash/-/lodash-4.17.21.tgz"]
        );
        assert_eq!(
            purge_artifact(dir, "@scope/pkg", "1.0.0"),
            ["/@scope/pkg/-/pkg-1.0.0.tgz"]
        );
        assert!(purge_artifact(dir, "lodash", "4.17.21").is_empty());
    }
}
