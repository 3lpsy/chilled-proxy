//! Test data and small helpers: metadata bodies and sentinel `Last-Modified`s.
#![allow(dead_code)]

use std::time::{Duration, SystemTime};

use chilled_core::http::fmt_http_date;

/// A `Last-Modified` that is always older than any plausible cooldown cutoff.
pub const OLD: &str = "Sat, 01 Jan 2000 00:00:00 GMT";
/// A slightly newer always-old `Last-Modified` (for latest/release ordering).
pub const OLD_NEWER: &str = "Mon, 01 Jan 2001 00:00:00 GMT";
/// A `Last-Modified` far in the future — always newer than the cutoff.
pub const TOO_NEW: &str = "Wed, 01 Jan 2999 00:00:00 GMT";

/// Opaque jar payload. The proxy treats artifact bytes as opaque, so only the
/// byte-for-byte round trip matters.
pub const JAR_BYTES: &[u8] = b"PK\x03\x04chilled-maven-test-jar-bytes\x00";

/// Builds a realistic `maven-metadata.xml` body.
pub fn metadata_xml(
    group: &str,
    artifact: &str,
    versions: &[&str],
    latest: &str,
    release: &str,
) -> String {
    let version_lines: String = versions
        .iter()
        .map(|v| format!("      <version>{v}</version>\n"))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>{group}</groupId>
  <artifactId>{artifact}</artifactId>
  <versioning>
    <latest>{latest}</latest>
    <release>{release}</release>
    <versions>
{version_lines}    </versions>
    <lastUpdated>20240101000000</lastUpdated>
  </versioning>
</metadata>
"#
    )
}

/// Formats `now + offset_secs` as an IMF-fixdate string. Used by the boundary
/// test, which needs a `Last-Modified` exactly at the cooldown cutoff.
pub fn http_date_from_now(offset_secs: i64) -> String {
    let now = SystemTime::now();
    let t = if offset_secs >= 0 {
        now + Duration::from_secs(offset_secs as u64)
    } else {
        now - Duration::from_secs(offset_secs.unsigned_abs())
    };
    fmt_http_date(t)
}
