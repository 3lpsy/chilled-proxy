use crate::builder::TestServerBuilder;
use axum::Router;
use std::time::{Duration, UNIX_EPOCH};

#[tokio::test]
async fn mocks_seeding_and_hit_counting() {
    let server = TestServerBuilder::new("/demo")
        .start(|_| Router::new())
        .await;

    server
        .mock_get("/pkg", b"body", &[("etag", "\"e1\"")])
        .await;
    server.mock_head("/pkg.head", 200, &[]).await;

    let client = reqwest::Client::new();
    let r = client
        .get(format!("{}/pkg", server.mock_upstream.uri()))
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["etag"], "\"e1\"");
    assert_eq!(r.text().await.unwrap(), "body");

    client
        .head(format!("{}/pkg.head", server.mock_upstream.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(server.upstream_hits("/pkg").await, 1);
    assert_eq!(server.upstream_total().await, 2);

    let mtime = UNIX_EPOCH + Duration::from_secs(1_000_000);
    server.seed_file("sub/dir/file", b"cached", mtime);
    assert_eq!(
        std::fs::read(server.cache_path("sub/dir/file")).unwrap(),
        b"cached"
    );
}
