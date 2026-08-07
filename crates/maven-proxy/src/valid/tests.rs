use super::{is_artifact_file, is_dir_segment, is_file_segment, is_version};

#[test]
fn dir_segments_accept_normal_names() {
    for seg in ["org", "commons-lang3", "1.0.0", "_internal", "guava", "a"] {
        assert!(is_dir_segment(seg), "segment: {seg}");
    }
}

#[test]
fn dir_segments_reject_hostile_names() {
    for seg in [
        "", ".", "..", ".hidden", "a b", "a:b", "a@b", "a/b", "a\\b", "a+b", "evil%2e",
    ] {
        assert!(!is_dir_segment(seg), "segment: {seg}");
    }
}

#[test]
fn file_segments_additionally_allow_plus() {
    assert!(is_file_segment("thing-1.0.0+b1.jar"));
    assert!(!is_file_segment(".hidden"));
    assert!(!is_file_segment("+lead"));
    assert!(!is_file_segment("a b.jar"));
}

#[test]
fn version_charset_and_bounds() {
    assert!(is_version("1.0.0"));
    assert!(is_version("1.0.0-rc1+b2"));
    assert!(is_version("20040616"));
    assert!(!is_version(""));
    assert!(!is_version(".1"));
    assert!(!is_version("1/0"));
    assert!(!is_version(&"9".repeat(129)));
}

#[test]
fn artifact_files_accept_whitelisted_shapes() {
    for file in [
        "lib-1.0.jar",
        "lib-1.0.pom",
        "lib-1.0.war",
        "lib-1.0.aar",
        "lib-1.0.module",
        "lib-1.0.zip",
        "lib-1.0.tar.gz",
        "lib-1.0.jar.sha1",
        "lib-1.0.jar.md5",
        "lib-1.0.jar.sha256",
        "lib-1.0.jar.sha512",
        "lib-1.0.jar.asc",
        "lib-1.0.jar.asc.sha1",
        "lib-1.0-sources.jar",
        "lib-1.0-javadoc.jar.sha1",
    ] {
        assert!(is_artifact_file("lib", "1.0", file), "file: {file}");
    }
}

#[test]
fn artifact_files_reject_mismatches() {
    for file in [
        "other-1.0.jar",   // wrong artifact
        "lib-2.0.jar",     // wrong version
        "lib-1.0.exe",     // disallowed extension
        "lib-1.0.jar.exe", // disallowed trailing extension
        "lib-1.0",         // no extension
        "lib-1.0-.jar",    // empty classifier
        "lib-1.0-a+b.jar", // classifier charset
        "lib-1.0x.jar",    // version not delimited
        "maven-metadata.xml",
    ] {
        assert!(!is_artifact_file("lib", "1.0", file), "file: {file}");
    }
}

#[test]
fn resolved_snapshot_artifacts_are_accepted() {
    // Under `thing/1.0-SNAPSHOT/`, Maven fetches timestamped build files.
    assert!(is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-20240101.120000-1.jar"
    ));
    assert!(is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-20240101.120000-1-sources.jar"
    ));
    assert!(is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-20240101.120000-1.jar.sha1"
    ));
    // The plain -SNAPSHOT form still works.
    assert!(is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-SNAPSHOT.jar"
    ));
}

#[test]
fn snapshot_form_does_not_loosen_validation() {
    // A different artifact, a non-timestamp build token, and traversal must
    // still be refused.
    assert!(!is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "other-1.0-20240101.120000-1.jar"
    ));
    assert!(!is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-evil.jar"
    ));
    assert!(!is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-2024.12-1.jar"
    ));
    // Traversal in the build token is refused.
    assert!(!is_artifact_file(
        "thing",
        "1.0-SNAPSHOT",
        "thing-1.0-../../etc-1.jar"
    ));
}
