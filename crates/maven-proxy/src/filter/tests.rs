use super::{filter_metadata, list_versions};
use crate::sidecar::{Stamp, VersionTimes};

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>com.example</groupId>
  <artifactId>thing</artifactId>
  <versioning>
    <latest>2.0.0</latest>
    <release>2.0.0</release>
    <versions>
      <version>1.0.0</version>
      <version>1.1.0</version>
      <version>2.0.0</version>
    </versions>
    <lastUpdated>20240101000000</lastUpdated>
  </versioning>
</metadata>
"#;

fn times(entries: &[(&str, u64)]) -> VersionTimes {
    let mut t = VersionTimes::default();
    for (v, ts) in entries {
        t.insert(
            (*v).to_owned(),
            Stamp {
                ts: *ts,
                src: "lm".to_owned(),
            },
        );
    }
    t
}

#[test]
fn lists_versions_in_order() {
    assert_eq!(
        list_versions(XML.as_bytes()).unwrap(),
        vec!["1.0.0", "1.1.0", "2.0.0"]
    );
}

#[test]
fn drops_versions_newer_than_cutoff_and_repoints() {
    let t = times(&[("1.0.0", 100), ("1.1.0", 200), ("2.0.0", 900)]);
    let out = filter_metadata(XML.as_bytes(), &t, 500).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();

    assert!(text.contains("<version>1.0.0</version>"));
    assert!(text.contains("<version>1.1.0</version>"));
    assert!(!text.contains("2.0.0"), "gated version fully gone");
    // latest/release repointed to the max-ts survivor.
    assert!(text.contains("<latest>1.1.0</latest>"));
    assert!(text.contains("<release>1.1.0</release>"));
    // Untouched elements and the XML declaration survive.
    assert!(text.starts_with("<?xml"));
    assert!(text.contains("<lastUpdated>20240101000000</lastUpdated>"));
    assert!(text.contains("<groupId>com.example</groupId>"));
}

#[test]
fn unknown_age_is_gated_fail_closed() {
    // 2.0.0 has no sidecar entry at all: it must be dropped.
    let t = times(&[("1.0.0", 100), ("1.1.0", 200)]);
    let out = filter_metadata(XML.as_bytes(), &t, 500).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("2.0.0"));
    assert!(text.contains("<latest>1.1.0</latest>"));
}

#[test]
fn nothing_surviving_yields_none() {
    let t = times(&[("1.0.0", 900), ("1.1.0", 901), ("2.0.0", 902)]);
    assert!(filter_metadata(XML.as_bytes(), &t, 500).unwrap().is_none());
}

#[test]
fn boundary_ts_equal_to_cutoff_survives() {
    let t = times(&[("1.0.0", 500), ("1.1.0", 501), ("2.0.0", 502)]);
    let out = filter_metadata(XML.as_bytes(), &t, 500).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("<version>1.0.0</version>"));
    assert!(!text.contains("1.1.0"));
    assert!(text.contains("<latest>1.0.0</latest>"));
}

#[test]
fn release_skips_snapshot_survivors() {
    let xml = XML.replace("2.0.0", "2.0.0-SNAPSHOT");
    let t = times(&[("1.0.0", 100), ("1.1.0", 200), ("2.0.0-SNAPSHOT", 300)]);
    let out = filter_metadata(xml.as_bytes(), &t, 500).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();
    // latest may be the snapshot, release must not be.
    assert!(text.contains("<latest>2.0.0-SNAPSHOT</latest>"));
    assert!(text.contains("<release>1.1.0</release>"));
}

#[test]
fn release_dropped_when_only_snapshots_survive() {
    let xml = r#"<metadata><versioning><latest>1-SNAPSHOT</latest>
<release>1-SNAPSHOT</release><versions><version>1-SNAPSHOT</version></versions>
</versioning></metadata>"#;
    let t = times(&[("1-SNAPSHOT", 100)]);
    let out = filter_metadata(xml.as_bytes(), &t, 500).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("<release>"));
    assert!(text.contains("<latest>1-SNAPSHOT</latest>"));
}

#[test]
fn top_level_version_element_is_not_filtered() {
    // Old metadata layouts carry a top-level <version>; only entries inside
    // <versions> are subject to gating.
    let xml = r#"<metadata><version>9.9.9</version><versioning>
<versions><version>1.0.0</version></versions></versioning></metadata>"#;
    let t = times(&[("1.0.0", 100)]);
    let out = filter_metadata(xml.as_bytes(), &t, 500).unwrap().unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("<version>9.9.9</version>"));
    assert_eq!(list_versions(xml.as_bytes()).unwrap(), vec!["1.0.0"]);
}

#[test]
fn malformed_xml_is_an_error() {
    assert!(filter_metadata(b"<metadata><oops", &times(&[]), 500).is_err());
}
