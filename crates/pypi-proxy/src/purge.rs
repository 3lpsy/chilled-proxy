//! Cache deletion, reported through `RegistryProxy::{purge_artifact,purge_all}`.

use std::path::{Path, PathBuf};

use crate::stats::file_version;

/// Matches the scan's depth cap.
const MAX_DEPTH: usize = 10;

/// Deletes every cached file of one release (wheels, sdists, `.metadata`
/// sidecars); returns the mount-relative paths that re-fetch them.
pub(crate) fn purge_artifact(files_dir: &Path, project: &str, version: &str) -> Vec<String> {
    let root = files_dir.join(project);
    let mut deleted = Vec::new();
    walk(&root, MAX_DEPTH, &mut |path, file_name| {
        if file_version(file_name) == version && std::fs::remove_file(path).is_ok() {
            if let Ok(rel) = path.strip_prefix(&root) {
                deleted.push(format!("/files/{project}/{}", rel.to_string_lossy()));
            }
        }
    });
    deleted
}

/// Deletes every cached distribution file (simple-index caches stay).
pub(crate) fn purge_all(files_dir: &Path) {
    let _ = std::fs::remove_dir_all(files_dir);
    let _ = std::fs::create_dir_all(files_dir);
}

fn walk(dir: &Path, depth: usize, visit: &mut impl FnMut(&PathBuf, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if depth > 0 {
                walk(&path, depth - 1, visit);
            }
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        visit(&path, &name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purges_all_files_of_one_release() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("requests/packages/aa")).unwrap();
        std::fs::write(dir.join("requests/requests-2.0.0-py3-none-any.whl"), b"x").unwrap();
        std::fs::write(
            dir.join("requests/requests-2.0.0-py3-none-any.whl.metadata"),
            b"x",
        )
        .unwrap();
        std::fs::write(dir.join("requests/packages/aa/requests-2.0.0.tar.gz"), b"x").unwrap();
        std::fs::write(dir.join("requests/requests-2.1.0-py3-none-any.whl"), b"x").unwrap();

        let mut paths = purge_artifact(dir, "requests", "2.0.0");
        paths.sort();
        assert_eq!(
            paths,
            [
                "/files/requests/packages/aa/requests-2.0.0.tar.gz",
                "/files/requests/requests-2.0.0-py3-none-any.whl",
                "/files/requests/requests-2.0.0-py3-none-any.whl.metadata",
            ]
        );
        assert!(dir
            .join("requests/requests-2.1.0-py3-none-any.whl")
            .exists());
    }
}
