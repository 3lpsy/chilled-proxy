use super::packument::filter_packument;
use super::{filter_bytes, FilterResult};
use serde_json::{json, Value};
use url::Url;

const OLD: &str = "2000-01-01T00:00:00Z";
const NEW: &str = "2020-06-01T00:00:00Z";
/// Unix seconds between `OLD` and `NEW`.
const CUTOFF: u64 = 1_500_000_000;

fn proxy_url() -> Url {
    Url::parse("http://localhost:3080/npm/").unwrap()
}

fn doc(versions: &[(&str, &str)], latest: &str) -> Value {
    let mut time = serde_json::Map::new();
    let mut vers = serde_json::Map::new();
    time.insert("created".into(), json!(OLD));
    time.insert("modified".into(), json!(NEW));
    for (v, t) in versions {
        time.insert((*v).into(), json!(t));
        vers.insert(
            (*v).into(),
            json!({"version": v, "dist": {"tarball": format!("https://up.test/pkg/-/pkg-{v}.tgz")}}),
        );
    }
    json!({"name": "pkg", "dist-tags": {"latest": latest}, "versions": vers, "time": time})
}

#[test]
fn removes_too_new_from_versions_and_time() {
    let mut d = doc(&[("1.0.0", OLD), ("2.0.0", NEW)], "2.0.0");
    let outcome = filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").unwrap();
    assert_eq!(outcome.removed, 1);
    assert!(d["versions"].get("1.0.0").is_some());
    assert!(d["versions"].get("2.0.0").is_none());
    assert!(d["time"].get("1.0.0").is_some());
    assert!(d["time"].get("2.0.0").is_none());
}

#[test]
fn boundary_keeps_at_cutoff() {
    // A stamp exactly at the cutoff is kept; only strictly newer is dropped.
    let stamp = "2017-07-14T02:40:00Z"; // == 1_500_000_000
    let mut d = doc(&[("1.0.0", stamp)], "1.0.0");
    let outcome = filter_packument(&mut d, Some(1_500_000_000), &proxy_url(), "pkg").unwrap();
    assert_eq!(outcome.removed, 0);
    assert!(d["versions"].get("1.0.0").is_some());
}

#[test]
fn unparseable_or_missing_stamp_is_kept() {
    let mut d = doc(&[("1.0.0", "not-a-date")], "1.0.0");
    d["versions"]["2.0.0"] = json!({"version": "2.0.0"}); // not in the time map
    let outcome = filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").unwrap();
    assert_eq!(outcome.removed, 0);
    assert!(d["versions"].get("1.0.0").is_some());
    assert!(d["versions"].get("2.0.0").is_some());
}

#[test]
fn created_and_modified_are_not_versions() {
    // `modified` is NEW (too new) but must never be treated as a version.
    let mut d = doc(&[("1.0.0", OLD)], "1.0.0");
    let outcome = filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").unwrap();
    assert_eq!(outcome.removed, 0);
    assert!(d["time"].get("created").is_some());
    assert!(d["time"].get("modified").is_some());
}

#[test]
fn dist_tags_dropped_and_latest_repointed() {
    let mut d = doc(
        &[
            ("1.0.0", OLD),
            ("1.5.0", "2010-01-01T00:00:00Z"),
            ("2.0.0", NEW),
        ],
        "2.0.0",
    );
    d["dist-tags"]["next"] = json!("2.0.0");
    d["dist-tags"]["stable"] = json!("1.0.0");
    filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").unwrap();
    // Tags at removed versions are gone; latest repoints to the newest survivor.
    assert!(d["dist-tags"].get("next").is_none());
    assert_eq!(d["dist-tags"]["stable"], "1.0.0");
    assert_eq!(d["dist-tags"]["latest"], "1.5.0");
}

#[test]
fn surviving_latest_is_left_alone() {
    let mut d = doc(&[("1.0.0", OLD), ("2.0.0", NEW)], "1.0.0");
    filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").unwrap();
    assert_eq!(d["dist-tags"]["latest"], "1.0.0");
}

#[test]
fn all_filtered_returns_none() {
    let mut d = doc(&[("2.0.0", NEW), ("3.0.0", NEW)], "3.0.0");
    assert!(filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").is_none());
}

#[test]
fn missing_versions_under_cooldown_returns_none() {
    let mut d = json!({"name": "pkg", "time": {}});
    assert!(filter_packument(&mut d, Some(CUTOFF), &proxy_url(), "pkg").is_none());
}

#[test]
fn no_cutoff_keeps_everything_but_rewrites() {
    let mut d = doc(&[("1.0.0", OLD), ("2.0.0", NEW)], "2.0.0");
    let outcome = filter_packument(&mut d, None, &proxy_url(), "pkg").unwrap();
    assert_eq!(outcome.removed, 0);
    assert_eq!(d["dist-tags"]["latest"], "2.0.0");
    assert_eq!(
        d["versions"]["2.0.0"]["dist"]["tarball"],
        "http://localhost:3080/npm/pkg/-/pkg-2.0.0.tgz"
    );
}

#[test]
fn scoped_rewrite_uses_full_name_and_unscoped_file() {
    let mut d = doc(&[("1.0.0", OLD)], "1.0.0");
    filter_packument(&mut d, None, &proxy_url(), "@scope/pkg").unwrap();
    assert_eq!(
        d["versions"]["1.0.0"]["dist"]["tarball"],
        "http://localhost:3080/npm/@scope/pkg/-/pkg-1.0.0.tgz"
    );
}

#[test]
fn out_of_charset_version_is_dropped() {
    // It cannot be rewritten to a servable path, and leaving the upstream URL
    // would route the download around the proxy's gate — so it goes.
    let mut d = doc(&[("1.0.0", OLD)], "1.0.0");
    d["versions"]["evil/../v"] = json!({"dist": {"tarball": "https://up.test/x.tgz"}});
    filter_packument(&mut d, None, &proxy_url(), "pkg").unwrap();
    assert!(d["versions"].get("evil/../v").is_none());
    assert!(!d.to_string().contains("up.test"));
}

#[test]
fn filter_bytes_roundtrips() {
    let body = serde_json::to_vec(&doc(&[("1.0.0", OLD), ("2.0.0", NEW)], "2.0.0")).unwrap();
    let FilterResult::Body(bytes) = filter_bytes(&body, Some(CUTOFF), &proxy_url(), "pkg") else {
        panic!("expected a filtered body");
    };
    let out: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(out["versions"].get("2.0.0").is_none());
    assert_eq!(
        out["versions"]["1.0.0"]["dist"]["tarball"],
        "http://localhost:3080/npm/pkg/-/pkg-1.0.0.tgz"
    );
}

#[test]
fn filter_bytes_flags_invalid_and_all_filtered() {
    assert!(matches!(
        filter_bytes(b"not json", None, &proxy_url(), "pkg"),
        FilterResult::Invalid
    ));
    let body = serde_json::to_vec(&doc(&[("2.0.0", NEW)], "2.0.0")).unwrap();
    assert!(matches!(
        filter_bytes(&body, Some(CUTOFF), &proxy_url(), "pkg"),
        FilterResult::AllFiltered
    ));
}

#[test]
fn unservable_versions_are_dropped_not_left_pointing_upstream() {
    // A version that cannot form a tarball path must not keep its upstream
    // URL — a client would fetch it directly, past the download gate.
    let mut doc = json!({
        "name": "lodash",
        "dist-tags": {"latest": "1.0.0/../evil"},
        "time": {
            "1.0.0": "2000-01-01T00:00:00Z",
            "1.0.0/../evil": "2000-01-01T00:00:00Z"
        },
        "versions": {
            "1.0.0": {"dist": {"tarball": "https://registry.npmjs.org/lodash/-/lodash-1.0.0.tgz"}},
            "1.0.0/../evil": {"dist": {"tarball": "https://evil.example/payload.tgz"}}
        }
    });
    let proxy = Url::parse("http://localhost:3080/npm/").unwrap();

    let outcome = filter_packument(&mut doc, None, &proxy, "lodash").unwrap();
    assert_eq!(outcome.removed, 1);
    assert!(doc["versions"].get("1.0.0/../evil").is_none());
    assert!(doc["time"].get("1.0.0/../evil").is_none());
    // The dangling dist-tag was repointed at the surviving version.
    assert_eq!(doc["dist-tags"]["latest"], "1.0.0");
    let served = doc["versions"]["1.0.0"]["dist"]["tarball"]
        .as_str()
        .unwrap();
    assert_eq!(
        served,
        "http://localhost:3080/npm/lodash/-/lodash-1.0.0.tgz"
    );
    // No upstream URL survives anywhere in the served document.
    assert!(!doc.to_string().contains("evil.example"));
}
