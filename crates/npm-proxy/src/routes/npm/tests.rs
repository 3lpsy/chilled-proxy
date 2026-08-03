use super::*;

fn pkg(scope: Option<&str>, name: &str) -> PackageRef {
    PackageRef::new(scope, name).unwrap()
}

#[test]
fn classifies_packuments() {
    assert_eq!(
        parse_request("/lodash"),
        Some(NpmRequest::Packument(pkg(None, "lodash")))
    );
    assert_eq!(
        parse_request("/@scope/pkg"),
        Some(NpmRequest::Packument(pkg(Some("scope"), "pkg")))
    );
    // npm clients send the scope separator percent-encoded; one decode unifies.
    assert_eq!(
        parse_request("/@scope%2fpkg"),
        Some(NpmRequest::Packument(pkg(Some("scope"), "pkg")))
    );
    assert_eq!(
        parse_request("/@scope%2Fpkg"),
        Some(NpmRequest::Packument(pkg(Some("scope"), "pkg")))
    );
}

#[test]
fn classifies_version_docs() {
    assert_eq!(
        parse_request("/lodash/4.17.21"),
        Some(NpmRequest::VersionDoc(
            pkg(None, "lodash"),
            "4.17.21".into()
        ))
    );
    assert_eq!(
        parse_request("/@scope/pkg/1.0.0-rc.1"),
        Some(NpmRequest::VersionDoc(
            pkg(Some("scope"), "pkg"),
            "1.0.0-rc.1".into()
        ))
    );
}

#[test]
fn classifies_tarballs() {
    assert_eq!(
        parse_request("/lodash/-/lodash-4.17.21.tgz"),
        Some(NpmRequest::Tarball(
            pkg(None, "lodash"),
            "lodash-4.17.21.tgz".into(),
            "4.17.21".into()
        ))
    );
    assert_eq!(
        parse_request("/@scope/pkg/-/pkg-1.0.0.tgz"),
        Some(NpmRequest::Tarball(
            pkg(Some("scope"), "pkg"),
            "pkg-1.0.0.tgz".into(),
            "1.0.0".into()
        ))
    );
}

#[test]
fn rejects_malformed_paths() {
    for path in [
        "",
        "/",
        "/..",
        "/.hidden",
        "/_private",
        "/http:",
        "/a/b/c",
        "/%2e%2e%2f",                // decodes to a traversal
        "/@scope%252fpkg",           // double-encoded: residual `%` after one decode
        "/@/pkg",                    // empty scope
        "/@.bad/pkg",                // leading dot in scope
        "/@scope",                   // scope without a name
        "/lodash/1.0%2F0",           // slash smuggled into a version
        "/lodash/-/other-1.0.0.tgz", // tarball name mismatch
        "/lodash/-/lodash-1.0.0.tar",
        "/lodash/-/..%2F..%2Fetc.tgz",
    ] {
        assert_eq!(parse_request(path), None, "path: {path}");
    }
}
