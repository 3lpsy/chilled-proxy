use super::*;

#[test]
fn valid_names_pass() {
    for name in [
        "requests",
        "Foo.Bar_baz",
        "a",
        "zope.interface",
        "flask-RESTful",
        "a1-b2",
    ] {
        assert!(is_valid_name(name), "name: {name}");
    }
}

#[test]
fn invalid_names_are_rejected() {
    let too_long = "a".repeat(129);
    for name in [
        "",
        "..",
        ".",
        ".hidden",
        "trailing.",
        "-leading",
        "trailing-",
        "_x",
        "a/b",
        "a b",
        "a@b",
        "a%2eb",
        "évil",
        too_long.as_str(),
    ] {
        assert!(!is_valid_name(name), "name: {name:?}");
    }
}

#[test]
fn name_length_boundary() {
    assert!(is_valid_name(&"a".repeat(128)));
    assert!(!is_valid_name(&"a".repeat(129)));
}

#[test]
fn normalize_collapses_separator_runs() {
    assert_eq!(normalize("Foo.Bar_baz"), "foo-bar-baz");
    assert_eq!(normalize("a--b__c..d"), "a-b-c-d");
    assert_eq!(normalize("-._x_.-"), "-x-");
    assert_eq!(normalize("requests"), "requests");
    assert_eq!(normalize("Django"), "django");
}

#[test]
fn filename_extension_gate() {
    assert!(is_valid_filename("foo-1.0.0-py3-none-any.whl"));
    assert!(is_valid_filename("foo-1.0.0.tar.gz"));
    assert!(is_valid_filename("foo-1.0.0.zip"));
    assert!(is_valid_filename("foo-1.0.0.tar.bz2"));
    assert!(is_valid_filename("foo-1.0.0.egg"));
    assert!(is_valid_filename("foo-1.0.0+local.whl"));
    assert!(!is_valid_filename("foo-1.0.0.exe"));
    assert!(!is_valid_filename("foo-1.0.0"));
    assert!(!is_valid_filename("foo 1.whl"));
    assert!(!is_valid_filename("a/b.whl"));
    assert!(!is_valid_filename(""));
}

#[test]
fn fhp_path_shape() {
    // PyPI's own layout.
    assert_eq!(
        validate_fhp_path("packages/aa/bb/ccdd/foo-1.0.0.whl"),
        Some("foo-1.0.0.whl")
    );
    // Other indexes use other layouts; a mount fronting one must still be able
    // to serve its files, so depth is bounded rather than pinned.
    assert_eq!(
        validate_fhp_path("whl/cpu/torch-2.10.0+cpu-cp312-cp312-linux_aarch64.whl"),
        Some("torch-2.10.0+cpu-cp312-cp312-linux_aarch64.whl")
    );
    assert_eq!(
        validate_fhp_path("simple/foo-1.0.0.whl"),
        Some("foo-1.0.0.whl")
    );

    for bad in [
        "foo-1.0.0.whl",                         // no directory segment
        "a/b/c/d/e/f/g/h/i/foo-1.0.0.whl",       // deeper than the bound
        "packages/../bb/cc/foo-1.0.0.whl",       // traversal segment
        "packages/aa/bb/cc/foo.exe",             // bad extension
        "packages/aa/b\\b/cc/foo-1.0.0.whl",     // backslash
        "packages/aa/bb/cc/",                    // empty filename
        "packages/aa/bb/cc/foo-1.0.0.whl/extra", // trailing junk
        "packages/a:a/bb/cc/foo-1.0.0.whl",      // scheme char
        "https://evil.test/foo-1.0.0.whl",       // absolute URL smuggle
        "packages//bb/cc/foo-1.0.0.whl",         // empty segment
    ] {
        assert_eq!(validate_fhp_path(bad), None, "path: {bad}");
    }
}
