use super::*;
use std::path::Path;
use std::time::UNIX_EPOCH;
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

#[test]
fn entry_equivalence_prefers_etag() {
    let mut a = NpmEntry::new();
    let mut b = NpmEntry::new();
    assert!(!a.is_equivalent(&b)); // no validators at all

    a.set_etag("\"abc\"");
    b.set_etag("\"abc\"");
    assert!(a.is_equivalent(&b));

    b.set_etag("\"other\"");
    assert!(!a.is_equivalent(&b));

    let mut c = NpmEntry::new();
    let mut d = NpmEntry::new();
    c.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    d.set_mtime(c.mtime().unwrap());
    assert!(c.is_equivalent(&d));
}

#[test]
fn entry_ttl_expiry() {
    let mut entry = NpmEntry::new();
    assert!(!entry.is_expired_with_ttl(&Duration::ZERO)); // never checked

    entry.set_last_updated();
    assert!(!entry.is_expired_with_ttl(&Duration::from_secs(3600)));
    std::thread::sleep(Duration::from_millis(5));
    assert!(entry.is_expired_with_ttl(&Duration::ZERO));
}

#[test]
fn entry_mtime_roundtrips_last_modified() {
    let mut entry = NpmEntry::new();
    entry.set_mtime(UNIX_EPOCH);
    assert_eq!(
        entry.last_modified().unwrap(),
        "Thu, 01 Jan 1970 00:00:00 GMT"
    );
}
