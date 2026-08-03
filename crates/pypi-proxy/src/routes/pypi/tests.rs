use super::*;

#[test]
fn classify_simple_routes() {
    assert_eq!(classify("/simple/"), Route::ProjectList);
    assert_eq!(classify("/simple"), Route::ProjectList);
    assert_eq!(
        classify("/simple/requests/"),
        Route::Project("requests".into())
    );
    // No trailing slash -> redirect to the canonical slash form.
    assert_eq!(
        classify("/simple/requests"),
        Route::Redirect("requests".into())
    );
    // Non-normalized name -> redirect to the normalized path.
    assert_eq!(
        classify("/simple/Foo.Bar_baz/"),
        Route::Redirect("foo-bar-baz".into())
    );
}

#[test]
fn classify_rejects_bad_simple_paths() {
    for path in [
        "/simple/../",
        "/simple/./",
        "/simple/.hidden/",
        "/simple/-leading/",
        "/simple/a b/",
        "/simple/a/b/",
        "/simple//",
        "/simplex",
        "/simple/a%2eb/",
    ] {
        assert_eq!(classify(path), Route::NotFound, "path: {path}");
    }
}

#[test]
fn classify_file_routes() {
    assert_eq!(
        classify("/files/foo/packages/aa/bb/cc/foo-1.0.0.whl"),
        Route::File {
            project: "foo".into(),
            fhp_path: "packages/aa/bb/cc/foo-1.0.0.whl".into(),
            filename: "foo-1.0.0.whl".into(),
        }
    );
}

#[test]
fn classify_rejects_bad_file_paths() {
    for path in [
        "/files/Foo/packages/aa/bb/cc/foo-1.0.0.whl", // non-normalized project
        "/files/foo/packages/aa/bb/foo-1.0.0.whl",    // short hash path
        "/files/foo/packages/aa/bb/cc/foo.exe",       // bad extension
        "/files/foo/packages/../bb/cc/foo-1.0.0.whl", // traversal
        "/files/foo/https://evil.com/foo-1.0.0.whl",  // absolute URL smuggle
        "/files/foo",                                 // no tail
        "/files/foo/",                                // empty tail
    ] {
        assert_eq!(classify(path), Route::NotFound, "path: {path}");
    }
}

#[test]
fn classify_everything_else_404() {
    for path in ["/", "/pypi", "/index/foo", "/simpleextra/x", "/files"] {
        assert_eq!(classify(path), Route::NotFound, "path: {path}");
    }
}

#[test]
fn json_simple_ctype_detection() {
    assert!(is_json_simple("application/vnd.pypi.simple.v1+json"));
    assert!(is_json_simple(
        "application/vnd.pypi.simple.v1+json; charset=utf-8"
    ));
    assert!(is_json_simple("Application/VND.pypi.simple.v1+JSON"));
    assert!(!is_json_simple("text/html"));
    assert!(!is_json_simple("application/json"));
    assert!(!is_json_simple(""));
}

#[test]
fn entry_validator_prefers_etag() {
    let mut entry = PypiEntry::new("requests");
    assert_eq!(entry_validator(&entry), "");
    entry.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
    assert_eq!(entry_validator(&entry), "Sun, 06 Nov 1994 08:49:37 GMT");
    entry.set_etag("\"abc\"");
    assert_eq!(entry_validator(&entry), "\"abc\"");
}
