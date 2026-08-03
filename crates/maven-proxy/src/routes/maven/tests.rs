use super::{classify, ctype_for};
use crate::checksum::ChecksumAlgo;
use crate::valid::MavenRequest;

#[test]
fn classifies_artifact_level_metadata() {
    let req = classify("/org/apache/commons/commons-lang3/maven-metadata.xml").unwrap();
    match req {
        MavenRequest::Metadata { coords, algo } => {
            assert_eq!(coords.to_string(), "org.apache.commons:commons-lang3");
            assert_eq!(algo, None);
        }
        other => panic!("wrong classification: {other:?}"),
    }
}

#[test]
fn classifies_metadata_checksums() {
    for (ext, algo) in [
        ("sha1", ChecksumAlgo::Sha1),
        ("md5", ChecksumAlgo::Md5),
        ("sha256", ChecksumAlgo::Sha256),
        ("sha512", ChecksumAlgo::Sha512),
    ] {
        let req = classify(&format!("/com/example/thing/maven-metadata.xml.{ext}")).unwrap();
        assert!(
            matches!(req, MavenRequest::Metadata { algo: Some(a), .. } if a == algo),
            "ext: {ext}"
        );
    }
}

#[test]
fn classifies_snapshot_metadata_as_passthrough() {
    let req = classify("/com/example/thing/1.0-SNAPSHOT/maven-metadata.xml").unwrap();
    assert_eq!(
        req,
        MavenRequest::SnapshotMetadata {
            rel: "com/example/thing/1.0-SNAPSHOT/maven-metadata.xml".to_owned()
        }
    );
    // Checksums of snapshot metadata pass through too.
    assert!(matches!(
        classify("/com/example/thing/1.0-SNAPSHOT/maven-metadata.xml.sha1").unwrap(),
        MavenRequest::SnapshotMetadata { .. }
    ));
}

#[test]
fn classifies_artifact_downloads() {
    let req = classify("/com/example/thing/1.0.0/thing-1.0.0.jar").unwrap();
    match req {
        MavenRequest::Artifact {
            coords,
            version,
            file,
        } => {
            assert_eq!(coords.to_string(), "com.example:thing");
            assert_eq!(version, "1.0.0");
            assert_eq!(file, "thing-1.0.0.jar");
        }
        other => panic!("wrong classification: {other:?}"),
    }
}

#[test]
fn classifies_artifact_checksums_and_signatures_as_artifacts() {
    for file in [
        "thing-1.0.0.jar.sha1",
        "thing-1.0.0.pom.asc",
        "thing-1.0.0.jar.asc.sha512",
    ] {
        assert!(
            matches!(
                classify(&format!("/com/example/thing/1.0.0/{file}")),
                Some(MavenRequest::Artifact { .. })
            ),
            "file: {file}"
        );
    }
}

#[test]
fn rejects_invalid_paths() {
    let vectors = [
        "/",
        "/maven-metadata.xml",                            // no group/artifact
        "/thing/maven-metadata.xml",                      // no group
        "/1.0-SNAPSHOT/maven-metadata.xml",               // snapshot dir with no group/artifact
        "/com/example/thing/1.0.0/other-1.0.0.jar",       // filename mismatch
        "/com/example/thing/1.0.0/thing-1.0.0.exe",       // extension not whitelisted
        "/com/example/thing/1.0.0/thing-1.0.0.jar/extra", // file used as dir
        "/com/example/../thing/maven-metadata.xml",       // traversal
        "/com/.hidden/thing/maven-metadata.xml",          // leading-dot segment
        "/com/example/thing/%2e%2e%2fmaven-metadata.xml", // encoded traversal
        "/com/exam%252fple/thing/maven-metadata.xml",     // double-encoded (residual %)
        "/com/exam\\ple/thing/maven-metadata.xml",        // backslash
        "/com/example/thing/.1/thing-.1.jar",             // leading-dot version
        "/com/example/thing/1.0.0/random.txt",            // not an artifact file
    ];
    for path in vectors {
        assert!(classify(path).is_none(), "path: {path}");
    }
}

#[test]
fn rejects_oversized_paths() {
    let long = format!("/com/{}/thing/maven-metadata.xml", "a".repeat(1100));
    assert!(classify(&long).is_none());

    let deep = format!("/{}/maven-metadata.xml", ["seg"; 40].join("/"));
    assert!(classify(&deep).is_none());
}

#[test]
fn content_types_by_extension() {
    assert_eq!(ctype_for("a-1.0.jar"), "application/java-archive");
    assert_eq!(ctype_for("a-1.0.pom"), "text/xml");
    assert_eq!(ctype_for("a-1.0.jar.sha1"), "text/plain");
    assert_eq!(ctype_for("a-1.0.war"), "application/octet-stream");
    assert_eq!(ctype_for("a-1.0.jar.asc"), "application/octet-stream");
}
