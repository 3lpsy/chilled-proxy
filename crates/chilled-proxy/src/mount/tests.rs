use super::*;

#[test]
fn normalizes_valid_paths() {
    assert_eq!(parse("/crates").unwrap(), "/crates");
    // A trailing slash is dropped so both spellings mean one mount.
    assert_eq!(parse("/crates/").unwrap(), "/crates");
    assert_eq!(parse("  /npm  ").unwrap(), "/npm");
    assert_eq!(parse("/a/b/c").unwrap(), "/a/b/c");
    assert_eq!(parse("/my-registry_1.0~x").unwrap(), "/my-registry_1.0~x");
}

#[test]
fn root_is_valid_in_both_spellings() {
    assert_eq!(parse("/").unwrap(), "/");
    assert_eq!(parse("//").unwrap(), "/");
}

#[test]
fn rejects_malformed_paths() {
    assert!(parse("crates").is_err()); // not absolute
    assert!(parse("").is_err());
    assert!(parse("/a//b").is_err()); // empty segment
    assert!(parse("/../etc").is_err());
    assert!(parse("/a/./b").is_err());
    assert!(parse("/a b").is_err()); // whitespace
    assert!(parse("/a?b").is_err()); // query delimiter
    assert!(parse("/a%2fb").is_err()); // percent-encoding
}

#[test]
fn root_requires_being_the_only_registry() {
    assert!(check(&[("npm", "/".into())]).is_ok());

    let err = check(&[("npm", "/".into()), ("pypi", "/pypi".into())]).unwrap_err();
    assert!(err.contains("only one enabled"), "unexpected: {err}");
    assert!(
        err.contains("pypi"),
        "names the conflicting registry: {err}"
    );
}

#[test]
fn duplicate_mounts_are_rejected() {
    let err = check(&[("npm", "/pkgs".into()), ("pypi", "/pkgs".into())]).unwrap_err();
    assert!(err.contains("both mounted at '/pkgs'"), "unexpected: {err}");
}

#[test]
fn reserved_prefixes_cannot_be_mounted_on() {
    for reserved in RESERVED {
        let err = check(&[("npm", (*reserved).to_owned())]).unwrap_err();
        assert!(err.contains("is reserved"), "unexpected: {err}");
    }
}

#[test]
fn reservation_covers_everything_underneath() {
    // The management plane and UI get whole subtrees, not just exact paths.
    for path in [
        "/api/v1",
        "/api/v1/registries",
        "/ui/assets",
        "/healthz/sub",
    ] {
        let err = check(&[("npm", path.to_owned())]).unwrap_err();
        assert!(
            err.contains("is reserved"),
            "{path} should be refused: {err}"
        );
    }
}

#[test]
fn similarly_spelled_paths_are_still_allowed() {
    // Only the reserved path itself and its subtree are off limits.
    for path in ["/apis", "/api-registry", "/uikit", "/metricsx"] {
        assert!(
            check(&[("npm", path.to_owned())]).is_ok(),
            "{path} should be allowed"
        );
    }
}

#[test]
fn distinct_mounts_pass() {
    assert!(check(&[
        ("crates", "/crates".into()),
        ("npm", "/npm".into()),
        ("pypi", "/py".into()),
        ("maven", "/m2".into()),
    ])
    .is_ok());
}
