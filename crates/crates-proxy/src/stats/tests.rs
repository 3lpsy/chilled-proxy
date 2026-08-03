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
