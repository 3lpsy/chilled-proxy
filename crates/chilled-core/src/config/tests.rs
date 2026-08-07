use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use url::Url;

use super::*;

fn settings(cooldown_secs: u64, overrides: &[&str]) -> RegistrySettings {
    RegistrySettings {
        cache_dir: PathBuf::from("/tmp/x"),
        cache_ttl: Duration::from_secs(3600),
        cooldown: Duration::from_secs(cooldown_secs),
        overrides: Arc::new(overrides.iter().map(|s| s.to_string()).collect()),
        restrict_downloads: false,
        proxy_url: Url::parse("http://localhost:3080/crates/").unwrap(),
        max_metadata_size: 0x400_0000,
        max_artifact_size: 0x1000_0000,
    }
}

#[test]
fn cutoff_none_when_cooldown_disabled() {
    assert_eq!(settings(0, &[]).cutoff_for("serde"), None);
    assert_eq!(settings(0, &[]).serve_window("serde"), None);
}

#[test]
fn cutoff_none_for_overridden_package() {
    let s = settings(86_400, &["serde"]);
    assert_eq!(s.cutoff_for("serde"), None);
    assert!(s.cutoff_for("tokio").is_some());
    assert_eq!(s.serve_window("tokio"), Some(86_400));
}

#[test]
fn cutoff_is_now_minus_cooldown() {
    let s = settings(1000, &[]);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let cutoff = s.cutoff_for("serde").unwrap();
    assert!(cutoff <= now - 1000 && cutoff >= now - 1002);
}

#[test]
fn log_level_normalizes() {
    assert_eq!(normalize_log_level(Some("debug".into())), "debug");
    assert_eq!(normalize_log_level(Some("  WARN ".into())), "warn");
    assert_eq!(normalize_log_level(Some("Off".into())), "off");
    assert_eq!(normalize_log_level(Some("verbose".into())), "info");
    assert_eq!(normalize_log_level(Some(String::new())), "info");
    assert_eq!(normalize_log_level(None), "info");
}

#[test]
fn overrides_parse_lowercased() {
    let set = parse_overrides("Serde, tokio ,,FOO\nbar");
    assert!(set.contains("serde"));
    assert!(set.contains("tokio"));
    assert!(set.contains("foo"));
    assert!(set.contains("bar"));
    assert_eq!(set.len(), 4);
    assert!(parse_overrides("").is_empty());
}

#[test]
fn parse_size_accepts_plain_bytes_and_units() {
    assert_eq!(parse_size("0"), Ok(0));
    assert_eq!(parse_size("268435456"), Ok(0x1000_0000));
    assert_eq!(parse_size("1k"), Ok(1024));
    assert_eq!(parse_size("512m"), Ok(512 * 1024 * 1024));
    assert_eq!(parse_size("2g"), Ok(2 * 1024 * 1024 * 1024));
    // Surrounding space is trimmed, like the other config parsers.
    assert_eq!(parse_size("  4m  "), Ok(4 * 1024 * 1024));
}

#[test]
fn parse_size_unit_spellings_are_all_binary() {
    // `1MB` must not be 1_000_000: it is meant to raise a MiB-denominated
    // default, and silently landing under it would be worse than an error.
    for spelling in ["1m", "1M", "1mb", "1MB", "1mib", "1MiB"] {
        assert_eq!(parse_size(spelling), Ok(1024 * 1024), "{spelling}");
    }
    for spelling in ["1b", "1B"] {
        assert_eq!(parse_size(spelling), Ok(1), "{spelling}");
    }
}

#[test]
fn parse_size_rejects_junk() {
    assert!(parse_size("").is_err());
    assert!(parse_size("abc").is_err());
    assert!(parse_size("512q").is_err());
    assert!(parse_size("m").is_err());
    assert!(parse_size("-1").is_err());
    // Overflow is reported, not wrapped.
    assert!(parse_size("99999999999999999999g").is_err());
}
