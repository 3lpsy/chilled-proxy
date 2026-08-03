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
