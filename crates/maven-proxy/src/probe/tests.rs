use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::coords::MavenCoords;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn valid_last_modified_becomes_lm_stamp() {
    let stamp = stamp_from_last_modified(Some("Sat, 01 Jan 2000 00:00:00 GMT"));
    assert_eq!(stamp.src, "lm");
    assert_eq!(stamp.ts, 946_684_800);
}

#[test]
fn missing_header_falls_back_to_first_seen_now() {
    let before = now_secs();
    let stamp = stamp_from_last_modified(None);
    assert_eq!(stamp.src, "fs");
    assert!(stamp.ts >= before && stamp.ts <= now_secs());
}

#[test]
fn unparseable_header_falls_back_to_first_seen_now() {
    let before = now_secs();
    let stamp = stamp_from_last_modified(Some("yesterday-ish"));
    assert_eq!(stamp.src, "fs");
    assert!(stamp.ts >= before && stamp.ts <= now_secs());
}

#[test]
fn future_dates_still_parse_as_lm() {
    // A future Last-Modified stays "lm" — the ts itself gates the version.
    let stamp = stamp_from_last_modified(Some("Wed, 01 Jan 2999 00:00:00 GMT"));
    assert_eq!(stamp.src, "lm");
    assert!(stamp.ts > now_secs());
}

#[tokio::test]
async fn malformed_upstream_version_is_never_probed() {
    // A `<version>` naming an absolute URL would otherwise redirect the probe
    // off the pinned upstream host via `Url::join`.
    let client = reqwest::Client::new();
    let upstream = Url::parse("http://127.0.0.1:1/").unwrap();
    let coords = MavenCoords::new(&["com", "example"], "thing");

    for hostile in ["http://evil.example/x", "../../etc/passwd", "a/b"] {
        let probed = probe_version(&client, &upstream, &coords, hostile).await;
        // Fail-closed: recorded as first-seen now, with no request made. A
        // rejected version is never reported absent — that would turn a hostile
        // version string into a plain 404 instead of a gated one.
        assert_eq!(probed.clone().stamp().src, FIRST_SEEN_SRC);
        assert!(matches!(probed, Probed::Stamped(_)));
    }
}

#[test]
fn provisional_stamps_are_retried_only_while_they_gate() {
    let mut times = VersionTimes::default();
    times.insert(
        "1.0.0".into(),
        Stamp {
            ts: 500,
            src: FIRST_SEEN_SRC.to_owned(),
        },
    );
    times.insert(
        "2.0.0".into(),
        Stamp {
            ts: 500,
            src: LAST_MODIFIED_SRC.to_owned(),
        },
    );

    // Guess newer than the cutoff -> still gating -> retry.
    assert!(needs_retry(&times, "1.0.0", 100));
    // Guess older than the cutoff -> no longer changes the outcome.
    assert!(!needs_retry(&times, "1.0.0", 900));
    // A real Last-Modified age is never re-probed.
    assert!(!needs_retry(&times, "2.0.0", 100));
}
