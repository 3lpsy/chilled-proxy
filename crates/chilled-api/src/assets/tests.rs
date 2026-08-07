use axum::http::header;

use super::serve::{file_response, has_extension, mime_for};

#[test]
fn extension_detection_drives_spa_fallback() {
    assert!(has_extension("chilled-ui_bg.wasm"));
    assert!(has_extension("nested/style.css"));
    assert!(!has_extension("mount/npm"));
    // Dotted mount names are routes, not files.
    assert!(!has_extension("mount/corp.io"));
    assert!(!has_extension(""));
}

#[test]
fn only_hashed_snippets_cache_immutably() {
    let entry = file_response("chilled-ui.js", b"x");
    let cache = entry.headers().get(header::CACHE_CONTROL).unwrap();
    assert_eq!(cache, "no-cache");
    let snippet = file_response("snippets/dioxus-web-abc123/inline0.js", b"x");
    let cache = snippet.headers().get(header::CACHE_CONTROL).unwrap();
    assert!(cache.to_str().unwrap().contains("immutable"));
}

#[test]
fn mime_table_covers_the_bundle() {
    assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
    assert_eq!(mime_for("app.wasm"), "application/wasm");
    assert_eq!(mime_for("chilled-ui.js"), "text/javascript");
    assert_eq!(mime_for("style.css"), "text/css");
    assert_eq!(mime_for("odd.bin"), "application/octet-stream");
}
