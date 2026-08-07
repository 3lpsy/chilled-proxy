use super::TestServerBuilder;
use axum::routing::get;
use axum::Router;
use std::time::Duration;

#[tokio::test]
async fn builder_wires_settings_and_serves_under_prefix() {
    let server = TestServerBuilder::new("/demo")
        .cooldown(Duration::from_secs(86_400))
        .restrict_downloads()
        .override_package("Serde")
        .start(|ctx| {
            assert_eq!(ctx.settings.cooldown, Duration::from_secs(86_400));
            assert!(ctx.settings.restrict_downloads);
            assert!(ctx.settings.overrides.contains("serde"));
            assert_eq!(
                ctx.settings.proxy_url.as_str(),
                "http://localhost:3080/demo/"
            );
            Router::new().route("/ping", get(|| async { "pong" }))
        })
        .await;

    let resp = server.get("/ping", &[]).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "pong");

    // Outside the prefix -> 404 from the wrapper.
    assert_eq!(server.get_raw("/ping").await.status(), 404);
}

#[tokio::test]
async fn dead_upstream_points_at_refused_port() {
    let server = TestServerBuilder::new("/demo")
        .dead_upstream()
        .start(|ctx| {
            assert_eq!(ctx.upstream.as_str(), "http://127.0.0.1:1/");
            Router::new()
        })
        .await;
    assert_eq!(server.upstream_total().await, 0);
}
