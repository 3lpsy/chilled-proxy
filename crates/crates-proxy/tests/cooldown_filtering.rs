//! The core feature: sparse-index age-gating, its ETag rewriting, overrides,
//! and the window-aware conditional-GET behavior.

mod common;

use chilled_testkit::{marker_prefix, shift_marker_bucket};
use common::StartProxy;
use common::{ndjson, rfc3339_from_now, TestProxy, OLD, TOO_NEW};

const WEEK_SECS: u64 = 7 * 86_400;

/// The marker prefix a 7-day-filtered `etag123` body carries (the trailing
/// cutoff bucket moves with the clock).
fn week_prefix() -> String {
    marker_prefix("\"etag123\"", WEEK_SECS)
}

#[tokio::test]
async fn filtering_hides_too_new_version() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let body = ndjson("serde", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)]);
    proxy
        .mock_index(
            "serde",
            &body,
            "\"etag123\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;

    let resp = proxy.get_index("serde", &[]).await;
    assert_eq!(resp.status(), 200);
    // Filtered entry: weak, cooldown-tagged ETag and NO Last-Modified.
    assert!(resp.headers()["etag"]
        .to_str()
        .unwrap()
        .starts_with(&week_prefix()));
    assert!(resp.headers().get("last-modified").is_none());

    let text = resp.text().await.unwrap();
    assert!(text.contains(r#""vers":"1.0.0""#), "old version kept");
    assert!(
        !text.contains(r#""vers":"2.0.0""#),
        "too-new version hidden"
    );
}

#[tokio::test]
async fn boundary_keeps_at_cutoff_drops_newer() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    // `kept` sits an hour older than the cutoff, `dropped` an hour newer — a
    // comfortable margin around `now - 7d` that the request can't cross.
    let kept = rfc3339_from_now(-(WEEK_SECS as i64) - 3600);
    let dropped = rfc3339_from_now(-(WEEK_SECS as i64) + 3600);
    let body = ndjson("serde", &[("1.0.0", &kept), ("2.0.0", &dropped)]);
    proxy
        .mock_index(
            "serde",
            &body,
            "\"etag123\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;

    let text = proxy.get_index("serde", &[]).await.text().await.unwrap();
    assert!(text.contains(r#""vers":"1.0.0""#), "<= cutoff kept");
    assert!(!text.contains(r#""vers":"2.0.0""#), "> cutoff dropped");
}

#[tokio::test]
async fn cooldown_disabled_serves_verbatim() {
    let proxy = TestProxy::builder().start_proxy().await; // cooldown = 0
    let body = ndjson("serde", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)]);
    proxy
        .mock_index(
            "serde",
            &body,
            "\"etag123\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;

    let resp = proxy.get_index("serde", &[]).await;
    assert_eq!(resp.status(), 200);
    // Unfiltered: the strong upstream ETag (no weak marker) and Last-Modified.
    assert_eq!(resp.headers()["etag"], "\"etag123\"");
    assert!(resp.headers().get("last-modified").is_some());

    let text = resp.text().await.unwrap();
    assert!(text.contains(r#""vers":"2.0.0""#), "all versions visible");
}

#[tokio::test]
async fn marked_etag_revalidation_yields_304() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let body = ndjson("serde", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)]);
    proxy
        .mock_index(
            "serde",
            &body,
            "\"etag123\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;
    proxy.mock_index_304("serde", "\"etag123\"").await;

    let first = proxy.get_index("serde", &[]).await;
    let marker = first.headers()["etag"].to_str().unwrap().to_owned();
    assert!(marker.starts_with(&week_prefix()));

    // Revalidate with the marked ETag: same window and bucket -> 304, echoed.
    let second = proxy
        .get_index("serde", &[("if-none-match", &marker)])
        .await;
    assert_eq!(second.status(), 304);
    assert_eq!(second.headers()["etag"], marker);
    assert!(second.text().await.unwrap().is_empty());
    // Served from the metadata cache, not re-fetched.
    assert_eq!(
        proxy
            .upstream_hits(&proxy.index_upstream_path("serde"))
            .await,
        1
    );
}

#[tokio::test]
async fn stale_bucket_marker_is_reserved_not_304() {
    // Regression: the marker must carry the cutoff bucket, not just the window
    // length. A client holding a copy filtered at an earlier bucket has to be
    // re-served, or versions that aged past the cooldown would stay invisible
    // to it for as long as upstream's own validator is unchanged.
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let body = ndjson("serde", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)]);
    proxy
        .mock_index(
            "serde",
            &body,
            "\"etag123\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;
    proxy.mock_index_304("serde", "\"etag123\"").await;

    let first = proxy.get_index("serde", &[]).await;
    let marker = first.headers()["etag"].to_str().unwrap().to_owned();

    // Same upstream validator, older filter bucket -> full body, not a 304.
    let stale = shift_marker_bucket(&marker, -1);
    let resp = proxy.get_index("serde", &[("if-none-match", &stale)]).await;
    assert_eq!(resp.status(), 200);
    assert!(resp.text().await.unwrap().contains(r#""vers":"1.0.0""#));
}

#[tokio::test]
async fn unmarked_etag_under_cooldown_is_not_304() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let body = ndjson("serde", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)]);
    proxy
        .mock_index(
            "serde",
            &body,
            "\"etag123\"",
            "Thu, 01 Jan 1970 00:00:00 GMT",
        )
        .await;

    proxy.get_index("serde", &[]).await;
    // A client whose validator lacks the current window marker must NOT get a
    // 304 — it gets the full filtered body instead.
    let resp = proxy
        .get_index("serde", &[("if-none-match", "\"etag123\"")])
        .await;
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(text.contains(r#""vers":"1.0.0""#));
    assert!(!text.contains(r#""vers":"2.0.0""#));
}

#[tokio::test]
async fn non_utf8_body_passes_through_unfiltered() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let raw = vec![0xff_u8, 0xfe, b'\n', 0x80, 0x00];
    proxy
        .mock_index_bytes("serde", raw.clone(), "\"etagX\"")
        .await;

    let resp = proxy.get_index("serde", &[]).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), raw.as_slice());
}
