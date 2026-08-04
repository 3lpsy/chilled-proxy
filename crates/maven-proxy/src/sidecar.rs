//! Persisted per-artifact version-age sidecar (`.chilled-versions.json`).
//!
//! Maven metadata carries no per-version timestamps, so ages are probed over
//! HTTP and persisted here: `{"1.2.3": {"ts": 1742440425, "src": "lm"}}` where
//! `src` is `"lm"` (from `Last-Modified`) or `"fs"` (first-seen fallback).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use log::{debug, error};
use serde_json::{json, Map, Value};

/// Distinguishes concurrent temp files written by this process.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// The sidecar file name inside the artifact cache directory.
pub(crate) const SIDECAR_FILE: &str = ".chilled-versions.json";

/// `src` value for an age read from a POM's `Last-Modified`.
pub(crate) const LAST_MODIFIED_SRC: &str = "lm";

/// `src` value for a first-seen fallback age (the probe failed).
pub(crate) const FIRST_SEEN_SRC: &str = "fs";

/// One recorded version age.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    /// Age timestamp (unix seconds).
    pub(crate) ts: u64,
    /// Source: `"lm"` (Last-Modified) or `"fs"` (first-seen fallback).
    pub(crate) src: String,
}

/// The per-artifact map of version → age stamp.
#[derive(Debug, Clone, Default)]
pub(crate) struct VersionTimes {
    map: BTreeMap<String, Stamp>,
}

impl VersionTimes {
    /// Loads a sidecar file; a missing or corrupt file yields an empty map.
    pub(crate) fn load(path: &Path) -> Self {
        let Ok(data) = std::fs::read(path) else {
            return VersionTimes::default();
        };
        match serde_json::from_slice::<Value>(&data) {
            Ok(Value::Object(obj)) => VersionTimes {
                map: obj.into_iter().filter_map(parse_entry).collect(),
            },
            _ => {
                debug!("cache: ignoring corrupt sidecar at {}", path.display());
                VersionTimes::default()
            }
        }
    }

    /// Writes the sidecar atomically (`.tmp` then rename). Errors are logged.
    pub(crate) fn save(&self, path: &Path) {
        let mut obj = Map::new();
        for (version, stamp) in &self.map {
            obj.insert(version.clone(), json!({"ts": stamp.ts, "src": stamp.src}));
        }
        let data = Value::Object(obj).to_string();

        let Some(parent) = path.parent() else {
            error!("cache: refusing to write a parentless sidecar path");
            return;
        };
        if let Err(e) = std::fs::create_dir_all(parent) {
            error!("cache: failed to create sidecar directory: {e}");
            return;
        }
        // A per-write temp name: a shared one lets a concurrent writer rename
        // a half-written file into place.
        let tmp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        if let Err(e) = std::fs::write(&tmp, data) {
            error!("cache: failed to write sidecar tmp file: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            error!("cache: failed to rename sidecar into place: {e}");
        }
    }

    /// The recorded age (unix seconds) of `version`, if known.
    pub(crate) fn get(&self, version: &str) -> Option<u64> {
        self.map.get(version).map(|s| s.ts)
    }

    /// The full recorded stamp of `version`, if known (test inspection).
    #[cfg(test)]
    pub(crate) fn stamp(&self, version: &str) -> Option<&Stamp> {
        self.map.get(version)
    }

    /// Whether the recorded age came from a failed probe (first-seen), which
    /// means it is a guess and worth re-probing while it still gates.
    pub(crate) fn is_provisional(&self, version: &str) -> bool {
        self.map
            .get(version)
            .is_some_and(|s| s.src == FIRST_SEEN_SRC)
    }

    /// Whether `version` has a recorded age.
    pub(crate) fn contains(&self, version: &str) -> bool {
        self.map.contains_key(version)
    }

    /// Records (or replaces) the age stamp for `version`.
    pub(crate) fn insert(&mut self, version: String, stamp: Stamp) {
        self.map.insert(version, stamp);
    }
}

/// Parses one `"version": {"ts": .., "src": ".."}` sidecar entry.
fn parse_entry((version, value): (String, Value)) -> Option<(String, Stamp)> {
    let obj = value.as_object()?;
    let ts = obj.get("ts")?.as_u64()?;
    let src = obj.get("src")?.as_str()?.to_owned();
    Some((version, Stamp { ts, src }))
}

#[cfg(test)]
mod tests {
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
}
