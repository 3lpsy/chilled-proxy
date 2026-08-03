//! Cooldown age-gating on the simple index: too-new files vanish from both
//! representations, versions are recomputed, and filtering is memoized.

mod common;

use common::{rfc3339_from_now, simple_json, TestProxy, OLD, SHA, SIMPLE_CTYPE, TOO_NEW};

const JSON_ACCEPT: &[(&str, &str)] = &[("accept", SIMPLE_CTYPE)];

#[tokio::test]
async fn too_new_file_dropped_from_json_and_html() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    let body = simple_json(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let resp = proxy.get_simple("foo", JSON_ACCEPT).await;
    assert_eq!(resp.status(), 200);
    // Cooldown marker (window + cutoff bucket) and format tag on the validator.
    let etag = resp.headers()["etag"].to_str().unwrap().to_owned();
    assert!(etag.starts_with(&chilled_testkit::marker_prefix("\"e1\"", 86_400)));
    assert!(etag.ends_with(".j\""));
    let doc: serde_json::Value = resp.json().await.unwrap();
    let files = doc["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], "foo-1.0.0.tar.gz");
    assert_eq!(doc["versions"], serde_json::json!(["1.0.0"]));

    let html = proxy.get_simple("foo", &[]).await.text().await.unwrap();
    assert!(html.contains("foo-1.0.0.tar.gz"));
    assert!(!html.contains("foo-2.0.0.tar.gz"));
}

#[tokio::test]
async fn upload_time_at_cutoff_boundary_is_kept() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    // Exactly one cooldown old == at the cutoff -> kept (only strictly newer drops).
    let at_cutoff = rfc3339_from_now(-86_400);
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", &at_cutoff, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
    assert_eq!(doc["versions"], serde_json::json!(["1.0.0"]));
}

#[tokio::test]
async fn just_inside_cooldown_is_dropped() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    // One hour inside the window -> dropped.
    let inside = rfc3339_from_now(-86_400 + 3_600);
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", &inside, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert!(doc["files"].as_array().unwrap().is_empty());
    assert_eq!(doc["versions"], serde_json::json!([]));
}

#[tokio::test]
async fn cooldown_off_keeps_everything() {
    let proxy = TestProxy::builder().start().await;
    let body = simple_json(
        "foo",
        &[
            ("foo-1.0.0.tar.gz", OLD, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 2);
    assert_eq!(doc["versions"], serde_json::json!(["1.0.0", "2.0.0"]));
}

#[tokio::test]
async fn missing_upload_time_dropped_only_under_cooldown() {
    // Empty upload-time in the fixture omits the key entirely.
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", "", SHA)]);

    let gated = TestProxy::builder().cooldown_days(1).start().await;
    gated.mock_simple("foo", &body, "\"e1\"").await;
    let doc: serde_json::Value = gated
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert!(doc["files"].as_array().unwrap().is_empty());

    let open = TestProxy::builder().start().await;
    open.mock_simple("foo", &body, "\"e1\"").await;
    let doc: serde_json::Value = open
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn version_survives_through_old_wheel_when_sdist_is_new() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    let body = simple_json(
        "foo",
        &[
            ("foo-1.0.0-py3-none-any.whl", OLD, SHA),
            ("foo-1.0.0.tar.gz", TOO_NEW, SHA),
            ("foo-2.0.0.tar.gz", TOO_NEW, SHA),
        ],
    );
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    // 1.0.0 survives via its wheel; 2.0.0 lost all its files.
    assert_eq!(doc["versions"], serde_json::json!(["1.0.0"]));
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn identical_serves_hit_upstream_once() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let first = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .text()
        .await
        .unwrap();
    let second = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        proxy
            .upstream_hits(&proxy.simple_upstream_path("foo"))
            .await,
        1
    );
}

#[tokio::test]
async fn plus_offset_upload_time_still_parses() {
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    // `+00:00` instead of `Z`: must parse (and pass the gate), not fail closed.
    let body = simple_json(
        "foo",
        &[("foo-1.0.0.tar.gz", "2000-01-01T00:00:00+00:00", SHA)],
    );
    proxy.mock_simple("foo", &body, "\"e1\"").await;

    let doc: serde_json::Value = proxy
        .get_simple("foo", JSON_ACCEPT)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn stale_bucket_marker_is_reserved_not_304() {
    // Regression: the marker carries the cutoff bucket, so a client holding a
    // copy filtered at an earlier bucket is re-served — otherwise files that
    // aged past the cooldown would stay invisible to it indefinitely.
    let proxy = TestProxy::builder().cooldown_days(1).start().await;
    let body = simple_json("foo", &[("foo-1.0.0.tar.gz", OLD, SHA)]);
    proxy.mock_simple("foo", &body, "\"e1\"").await;
    proxy.mock_simple_304("foo", "\"e1\"").await;

    let first = proxy.get_simple("foo", JSON_ACCEPT).await;
    let marker = first.headers()["etag"].to_str().unwrap().to_owned();

    // Same validator and format, older filter bucket -> full body, not a 304.
    let stale = chilled_testkit::shift_marker_bucket(&marker, -1);
    let mut headers = JSON_ACCEPT.to_vec();
    headers.push(("if-none-match", &stale));
    let resp = proxy.get_simple("foo", &headers).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["files"].as_array().unwrap().len(), 1);
}
