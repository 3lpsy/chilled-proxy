//! SSRF / traversal vectors: every malformed or hostile path is rejected with
//! `404` before any upstream request is made.

mod common;

use common::TestProxy;

#[tokio::test]
async fn hostile_paths_never_reach_upstream() {
    let proxy = TestProxy::builder().start().await;

    // Literal `..` segments are normalized away by HTTP clients before they
    // ever reach the wire; the server-side rejection is unit-tested in
    // `routes::maven::tests` against `classify` directly.
    let vectors = [
        // Encoded traversal and double-encoding.
        "/com/%2e%2e/thing/maven-metadata.xml",
        "/com/%252e%252e/thing/maven-metadata.xml",
        // Backslash and control-byte smuggling.
        "/com/example%5Cthing/maven-metadata.xml",
        "/com/example%00/thing/maven-metadata.xml",
        // Leading-dot segments (hidden files, sidecar access).
        "/com/example/.thing/maven-metadata.xml",
        "/com/example/thing/1.0.0/.chilled-versions.json",
        // Filename not matching {artifact}-{version}.
        "/com/example/thing/1.0.0/other-1.0.0.jar",
        "/com/example/thing/1.0.0/thing-2.0.0.jar",
        // Disallowed extension.
        "/com/example/thing/1.0.0/thing-1.0.0.exe",
        // Missing structure.
        "/maven-metadata.xml",
        "/thing-1.0.0.jar",
    ];
    for path in vectors {
        let status = proxy.get(path).await.status();
        assert_eq!(status, 404, "path not rejected: {path}");
    }

    // Over the segment cap.
    let deep = format!("{}/thing/maven-metadata.xml", "/a".repeat(33));
    assert_eq!(proxy.get(&deep).await.status(), 404);

    // Over the length cap.
    let long = format!("/com/{}/maven-metadata.xml", "a".repeat(1100));
    assert_eq!(proxy.get(&long).await.status(), 404);

    assert_eq!(proxy.upstream_total().await, 0, "zero upstream contact");
}

#[tokio::test]
async fn non_read_methods_are_rejected() {
    let proxy = TestProxy::builder().start().await;

    let client = reqwest::Client::new();
    let url = format!(
        "{}/maven/com/example/thing/maven-metadata.xml",
        proxy.server.base_url
    );
    // A read-only surface: 405 with `Allow`, matching the sibling proxies.
    for resp in [
        client.post(&url).send().await.unwrap(),
        client.put(&url).send().await.unwrap(),
    ] {
        assert_eq!(resp.status(), 405);
        assert_eq!(resp.headers()["allow"], "GET, HEAD");
    }
    assert_eq!(proxy.upstream_total().await, 0);
}
