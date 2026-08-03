use super::{Stamp, VersionTimes, SIDECAR_FILE};

fn stamp(ts: u64, src: &str) -> Stamp {
    Stamp {
        ts,
        src: src.to_owned(),
    }
}

#[test]
fn save_load_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("g/a").join(SIDECAR_FILE);

    let mut times = VersionTimes::default();
    times.insert("1.2.3".into(), stamp(1_742_440_425, "lm"));
    times.insert("9.9.9".into(), stamp(1_754_000_000, "fs"));
    times.save(&path);

    let loaded = VersionTimes::load(&path);
    assert_eq!(loaded.get("1.2.3"), Some(1_742_440_425));
    assert_eq!(loaded.stamp("9.9.9"), Some(&stamp(1_754_000_000, "fs")));
    assert!(!loaded.contains("2.0.0"));
    // The tmp file was renamed away.
    assert!(!path.with_extension("json.tmp").exists());
}

#[test]
fn missing_file_loads_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let times = VersionTimes::load(&tmp.path().join("nope.json"));
    assert!(!times.contains("1.0.0"));
}

#[test]
fn corrupt_file_loads_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(SIDECAR_FILE);
    std::fs::write(&path, b"{ not json").unwrap();
    assert!(!VersionTimes::load(&path).contains("1.0.0"));

    std::fs::write(&path, b"[1,2,3]").unwrap();
    assert!(!VersionTimes::load(&path).contains("1.0.0"));
}

#[test]
fn malformed_entries_are_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(SIDECAR_FILE);
    std::fs::write(
        &path,
        br#"{"good": {"ts": 5, "src": "lm"}, "bad": {"ts": "x"}, "worse": 7}"#,
    )
    .unwrap();
    let times = VersionTimes::load(&path);
    assert_eq!(times.get("good"), Some(5));
    assert!(!times.contains("bad"));
    assert!(!times.contains("worse"));
}

#[test]
fn insert_replaces_existing() {
    let mut times = VersionTimes::default();
    times.insert("1.0.0".into(), stamp(1, "fs"));
    times.insert("1.0.0".into(), stamp(2, "lm"));
    assert_eq!(times.get("1.0.0"), Some(2));
}
