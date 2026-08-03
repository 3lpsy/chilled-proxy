use super::*;

#[test]
fn scans_scoped_and_unscoped_tarballs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();

    std::fs::create_dir_all(dir.join("lodash")).unwrap();
    std::fs::write(dir.join("lodash/lodash-4.17.21.tgz"), b"x").unwrap();
    std::fs::write(dir.join("lodash/lodash-4.17.20.tgz"), b"x").unwrap();
    std::fs::create_dir_all(dir.join("@scope/pkg")).unwrap();
    std::fs::write(dir.join("@scope/pkg/pkg-1.0.0.tgz"), b"x").unwrap();

    // Malformed entries are skipped, not fatal.
    std::fs::write(dir.join("lodash/garbage.txt"), b"x").unwrap();
    std::fs::write(dir.join("lodash/other-1.0.0.tgz"), b"x").unwrap();
    std::fs::create_dir_all(dir.join(".bad name!")).unwrap();
    std::fs::write(dir.join(".bad name!/x-1.0.0.tgz"), b"x").unwrap();
    std::fs::create_dir_all(dir.join("@.badscope/pkg")).unwrap();
    std::fs::write(dir.join("@.badscope/pkg/pkg-1.0.0.tgz"), b"x").unwrap();

    let stats = cache_stats(dir);
    let got: Vec<_> = stats
        .artifacts
        .iter()
        .map(|a| format!("{}@{}", a.name, a.version))
        .collect();
    assert_eq!(
        got,
        ["@scope/pkg@1.0.0", "lodash@4.17.20", "lodash@4.17.21"]
    );
    assert!(stats.artifacts.iter().all(|a| a.cached_at > 0));
}

#[test]
fn missing_dir_yields_empty_stats() {
    let stats = cache_stats(Path::new("/definitely/not/here"));
    assert!(stats.artifacts.is_empty());
}
