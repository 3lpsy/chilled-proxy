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
