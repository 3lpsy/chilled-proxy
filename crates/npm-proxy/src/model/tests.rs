use super::*;
use std::path::Path;
use url::Url;

#[test]
fn package_refs_validate() {
    assert!(PackageRef::new(None, "lodash").is_some());
    assert!(PackageRef::new(Some("scope"), "pkg").is_some());
    assert!(PackageRef::new(None, ".hidden").is_none());
    assert!(PackageRef::new(Some(".bad"), "pkg").is_none());
    assert!(PackageRef::new(Some("scope"), "a/b").is_none());
    // Combined `@scope/name` length is capped at 214.
    assert!(PackageRef::new(Some(&"s".repeat(110)), &"n".repeat(110)).is_none());
    assert!(PackageRef::new(Some(&"s".repeat(106)), &"n".repeat(106)).is_some());
}

#[test]
fn names_and_paths() {
    let plain = PackageRef::new(None, "lodash").unwrap();
    assert_eq!(plain.full_name(), "lodash");
    assert_eq!(plain.unscoped(), "lodash");
    assert_eq!(plain.packument_rel(), Path::new("lodash"));
    assert_eq!(
        plain.tarball_rel("lodash-1.0.0.tgz"),
        Path::new("lodash/lodash-1.0.0.tgz")
    );

    let scoped = PackageRef::new(Some("scope"), "pkg").unwrap();
    assert_eq!(scoped.full_name(), "@scope/pkg");
    assert_eq!(scoped.unscoped(), "pkg");
    assert_eq!(scoped.packument_rel(), Path::new("@scope/pkg"));
    assert_eq!(
        scoped.tarball_rel("pkg-1.0.0.tgz"),
        Path::new("@scope/pkg/pkg-1.0.0.tgz")
    );
}

#[test]
fn upstream_urls_keep_host_and_path() {
    let base = Url::parse("http://upstream.test/").unwrap();
    let scoped = PackageRef::new(Some("scope"), "pkg").unwrap();

    let url = base.join(&scoped.upstream_packument_rel()).unwrap();
    assert_eq!(url.as_str(), "http://upstream.test/@scope/pkg");

    let url = base
        .join(&scoped.upstream_tarball_rel("pkg-1.0.0.tgz"))
        .unwrap();
    assert_eq!(
        url.as_str(),
        "http://upstream.test/@scope/pkg/-/pkg-1.0.0.tgz"
    );

    let plain = PackageRef::new(None, "lodash").unwrap();
    let url = base
        .join(&plain.upstream_tarball_rel("lodash-1.0.0.tgz"))
        .unwrap();
    assert_eq!(
        url.as_str(),
        "http://upstream.test/lodash/-/lodash-1.0.0.tgz"
    );
}
