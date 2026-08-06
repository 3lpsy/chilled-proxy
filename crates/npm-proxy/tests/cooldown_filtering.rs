//! The core feature: packument age-gating, dist-tag repair, its ETag marker,
//! memoization, and overrides.

mod common;

use common::StartProxy;
use common::{rfc3339_from_now, TestProxy, OLD, TOO_NEW};

const WEEK_SECS: u64 = 7 * 86_400;
/// The marker prefix a 7-day-filtered `etag123` packument carries (the
/// trailing cutoff bucket moves with the clock).
fn week_prefix() -> String {
    chilled_testkit::marker_prefix("\"etag123\"", 604_800)
}

#[tokio::test]
async fn filtering_hides_too_new_version_from_versions_and_time() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let resp = proxy.get_packument("lodash", &[]).await;
    assert_eq!(resp.status(), 200);
    // Filtered body: weak, cooldown-tagged ETag and no Last-Modified.
    assert!(resp.headers()["etag"]
        .to_str()
        .unwrap()
        .starts_with(&week_prefix()));
    assert!(resp.headers().get("last-modified").is_none());

    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(
        served["versions"].get("1.0.0").is_some(),
        "old version kept"
    );
    assert!(served["versions"].get("2.0.0").is_none(), "too-new hidden");
    assert!(served["time"].get("1.0.0").is_some());
    assert!(served["time"].get("2.0.0").is_none(), "time entry pruned");
}

#[tokio::test]
async fn boundary_keeps_at_cutoff_drops_newer() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    // `kept` sits an hour older than the cutoff, `dropped` an hour newer — a
    // comfortable margin around `now - 7d` that the request can't cross.
    let kept = rfc3339_from_now(-(WEEK_SECS as i64) - 3600);
    let dropped = rfc3339_from_now(-(WEEK_SECS as i64) + 3600);
    proxy
        .mock_packument("lodash", &[("1.0.0", &kept), ("2.0.0", &dropped)])
        .await;

    let served: serde_json::Value = proxy
        .get_packument("lodash", &[])
        .await
        .json()
        .await
        .unwrap();
    assert!(served["versions"].get("1.0.0").is_some(), "<= cutoff kept");
    assert!(
        served["versions"].get("2.0.0").is_none(),
        "> cutoff dropped"
    );
}

#[tokio::test]
async fn cooldown_disabled_keeps_everything_but_still_rewrites() {
    let proxy = TestProxy::builder().start_proxy().await; // cooldown = 0
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let resp = proxy.get_packument("lodash", &[]).await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(
        served["versions"].get("2.0.0").is_some(),
        "all versions visible"
    );
    assert_eq!(
        served["versions"]["2.0.0"]["dist"]["tarball"],
        "http://localhost:3080/npm/lodash/-/lodash-2.0.0.tgz"
    );
}

#[tokio::test]
async fn latest_repointed_to_newest_survivor() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    // latest = 3.0.0 (last entry) gets filtered; 2.0.0 is the newest survivor.
    proxy
        .mock_packument(
            "lodash",
            &[
                ("1.0.0", OLD),
                ("2.0.0", "2010-06-01T00:00:00Z"),
                ("3.0.0", TOO_NEW),
            ],
        )
        .await;

    let served: serde_json::Value = proxy
        .get_packument("lodash", &[])
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(served["dist-tags"]["latest"], "2.0.0");
}

#[tokio::test]
async fn tag_pointing_at_filtered_version_is_dropped() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    let body = common::packument_with_tags(
        "lodash",
        &[("2.0.0", TOO_NEW), ("1.0.0", OLD)],
        &[("next", "2.0.0"), ("stable", "1.0.0")],
        &proxy.upstream_url(),
    );
    proxy.mock_packument_body("lodash", &body).await;

    let served: serde_json::Value = proxy
        .get_packument("lodash", &[])
        .await
        .json()
        .await
        .unwrap();
    assert!(
        served["dist-tags"].get("next").is_none(),
        "dead tag dropped"
    );
    assert_eq!(served["dist-tags"]["stable"], "1.0.0");
    assert_eq!(served["dist-tags"]["latest"], "1.0.0");
}

#[tokio::test]
async fn all_versions_filtered_is_npm_404() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy.mock_packument("lodash", &[("2.0.0", TOO_NEW)]).await;

    let resp = proxy.get_packument("lodash", &[]).await;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "Not found");
}

#[tokio::test]
async fn filtered_body_is_memoized() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let first = proxy
        .get_packument("lodash", &[])
        .await
        .text()
        .await
        .unwrap();
    let second = proxy
        .get_packument("lodash", &[])
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(first, second, "identical filtered bodies");
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
}

#[tokio::test]
async fn marked_etag_revalidation_yields_304() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let first = proxy.get_packument("lodash", &[]).await;
    let marker = first.headers()["etag"].to_str().unwrap().to_owned();
    assert!(marker.starts_with(&week_prefix()));

    // Revalidate with the marked ETag: same window and bucket -> 304, echoed.
    let second = proxy
        .get_packument("lodash", &[("if-none-match", &marker)])
        .await;
    assert_eq!(second.status(), 304);
    assert_eq!(second.headers()["etag"], marker);
    assert_eq!(proxy.upstream_hits("/lodash").await, 1);
}

#[tokio::test]
async fn stale_bucket_marker_is_reserved_not_304() {
    // Regression: the marker carries the cutoff bucket, so a client holding a
    // copy filtered at an earlier bucket is re-served — otherwise versions
    // that aged past the cooldown would stay invisible to it indefinitely.
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let first = proxy.get_packument("lodash", &[]).await;
    let marker = first.headers()["etag"].to_str().unwrap().to_owned();

    let stale = chilled_testkit::shift_marker_bucket(&marker, -1);
    let resp = proxy
        .get_packument("lodash", &[("if-none-match", &stale)])
        .await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert!(doc["versions"]["1.0.0"].is_object());
}

#[tokio::test]
async fn unmarked_etag_under_cooldown_is_not_304() {
    let proxy = TestProxy::builder().cooldown_days(7).start_proxy().await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    proxy.get_packument("lodash", &[]).await;
    // A client whose validator lacks the current window marker must NOT get a
    // 304 — it gets the full filtered body instead.
    let resp = proxy
        .get_packument("lodash", &[("if-none-match", "\"etag123\"")])
        .await;
    assert_eq!(resp.status(), 200);
    let served: serde_json::Value = resp.json().await.unwrap();
    assert!(served["versions"].get("2.0.0").is_none());
}

#[tokio::test]
async fn override_package_is_exempt_from_cooldown() {
    let proxy = TestProxy::builder()
        .cooldown_days(7)
        .override_package("lodash")
        .start_proxy()
        .await;
    proxy
        .mock_packument("lodash", &[("1.0.0", OLD), ("2.0.0", TOO_NEW)])
        .await;

    let served: serde_json::Value = proxy
        .get_packument("lodash", &[])
        .await
        .json()
        .await
        .unwrap();
    assert!(
        served["versions"].get("2.0.0").is_some(),
        "override exempts"
    );
}
