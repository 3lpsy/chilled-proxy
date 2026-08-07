//! Sparse-index age-gating filter (crates.io NDJSON).
//!
//! Drops version lines whose `pubtime` is newer than a cutoff; `pubtime` is
//! read with a targeted byte scan, so large `deps` arrays are never parsed.

use chilled_core::time::parse_rfc3339z;

/// Filter raw sparse-index bytes, dropping any version line whose `pubtime` is
/// newer than `cutoff` (unix seconds). Non-UTF-8 bodies pass through unchanged.
pub(crate) fn filter_index(data: &[u8], cutoff: u64) -> Vec<u8> {
    match std::str::from_utf8(data) {
        Ok(body) => filter_body(body, cutoff),
        Err(_) => data.to_vec(),
    }
}

/// Extract the `pubtime` (unix seconds) of a specific `version` from a
/// sparse-index body. Used by `--restrict-downloads` to age-gate downloads.
/// Matches the compact `"vers":"<version>"` token (closing quote included).
pub(crate) fn version_pubtime(body: &str, version: &str) -> Option<u64> {
    let needle = format!("\"vers\":\"{version}\"");
    body.lines()
        .find(|line| line.contains(&needle))
        .and_then(line_pubtime_secs)
}

/// Walk the body line by line, dropping lines with `pubtime > cutoff`. Lines
/// without a `pubtime` (blanks, malformed) are kept verbatim, newlines preserved.
fn filter_body(body: &str, cutoff: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.is_empty() {
            out.extend_from_slice(line.as_bytes());
            continue;
        }
        match line_pubtime_secs(trimmed) {
            Some(secs) if secs > cutoff => {}
            _ => out.extend_from_slice(line.as_bytes()),
        }
    }
    out
}

/// Extract the `pubtime` field from one JSON line as unix seconds.
///
/// Scans for the `"pubtime"` *key* (followed by a colon, so a string value
/// reading `pubtime` is skipped) and reads its quoted timestamp.
fn line_pubtime_secs(line: &str) -> Option<u64> {
    let mut start = 0;
    while let Some(rel) = line[start..].find("\"pubtime\"") {
        let after = line[start + rel + "\"pubtime\"".len()..].trim_start();
        match after.strip_prefix(':') {
            Some(rest) => {
                let rest = rest.trim_start().strip_prefix('"')?;
                let end = rest.find('"')?;
                return parse_rfc3339z(&rest[..end]);
            }
            // This occurrence was a string value, not a key — keep looking.
            None => start += rel + "\"pubtime\"".len(),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_drops_too_new() {
        let body = concat!(
            r#"{"name":"a","vers":"1","pubtime":"2026-01-01T00:00:00Z"}"#,
            "\n",
            r#"{"name":"a","vers":"2","pubtime":"2026-03-20T00:00:00Z"}"#,
            "\n",
        );
        // cutoff = 2026-02-01: the 03-20 release is newer → dropped.
        let cutoff = parse_rfc3339z("2026-02-01T00:00:00Z").unwrap();
        let out = String::from_utf8(filter_body(body, cutoff)).unwrap();
        assert!(out.contains(r#""vers":"1""#));
        assert!(!out.contains(r#""vers":"2""#));
    }

    #[test]
    fn filter_keeps_lines_without_pubtime() {
        // Blank lines, lines with no pubtime, and a missing trailing newline are
        // all preserved verbatim, regardless of cutoff.
        let body = "\n{\"name\":\"a\",\"vers\":\"1\"}\nnot json";
        let out = String::from_utf8(filter_body(body, 0)).unwrap();
        assert_eq!(out, body);
    }

    #[test]
    fn filter_preserves_crlf_endings() {
        let body = concat!(
            "{\"vers\":\"1\",\"pubtime\":\"2026-01-01T00:00:00Z\"}\r\n",
            "{\"vers\":\"2\",\"pubtime\":\"2026-03-20T00:00:00Z\"}\r\n",
        );
        let cutoff = parse_rfc3339z("2026-02-01T00:00:00Z").unwrap();
        let out = String::from_utf8(filter_body(body, cutoff)).unwrap();
        // The kept line retains its CRLF; the too-new line is dropped whole.
        assert_eq!(
            out,
            "{\"vers\":\"1\",\"pubtime\":\"2026-01-01T00:00:00Z\"}\r\n"
        );
    }

    #[test]
    fn filter_keeps_line_at_cutoff_boundary() {
        // Only strictly-newer-than-cutoff is dropped; pubtime == cutoff stays.
        let pubtime = "2026-03-20T00:00:00Z";
        let cutoff = parse_rfc3339z(pubtime).unwrap();
        let body = format!("{{\"vers\":\"1\",\"pubtime\":\"{pubtime}\"}}\n");
        let out = String::from_utf8(filter_body(&body, cutoff)).unwrap();
        assert_eq!(out, body);
        // One second older a cutoff and the same line is dropped.
        assert!(String::from_utf8(filter_body(&body, cutoff - 1))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn filter_index_passes_through_non_utf8() {
        // Invalid UTF-8 is returned untouched rather than mangled.
        let data = [0xff, 0xfe, 0x00, 0x01];
        assert_eq!(filter_index(&data, 0), data.to_vec());
    }

    #[test]
    fn version_pubtime_finds_exact_version() {
        let body = concat!(
            r#"{"name":"a","vers":"1.0.0","pubtime":"2026-01-01T00:00:00Z"}"#,
            "\n",
            r#"{"name":"a","vers":"1.0.1","pubtime":"2026-03-20T00:00:00Z"}"#,
            "\n",
        );
        assert_eq!(
            version_pubtime(body, "1.0.1"),
            parse_rfc3339z("2026-03-20T00:00:00Z")
        );
        assert_eq!(version_pubtime(body, "9.9.9"), None);
    }

    #[test]
    fn version_pubtime_requires_full_version_match() {
        // The closing quote in the needle prevents `1.0` matching `1.0.1`.
        let body = r#"{"name":"a","vers":"1.0.1","pubtime":"2026-03-20T00:00:00Z"}"#;
        assert_eq!(version_pubtime(body, "1.0"), None);
        assert_eq!(
            version_pubtime(body, "1.0.1"),
            parse_rfc3339z("2026-03-20T00:00:00Z")
        );
    }

    #[test]
    fn version_pubtime_none_without_pubtime() {
        let body = r#"{"name":"a","vers":"1.0.0"}"#;
        assert_eq!(version_pubtime(body, "1.0.0"), None);
    }

    #[test]
    fn line_with_pubtime() {
        let line = r#"{"name":"a","vers":"1","pubtime":"2026-03-20T03:13:45Z"}"#;
        assert_eq!(
            line_pubtime_secs(line),
            parse_rfc3339z("2026-03-20T03:13:45Z")
        );
    }

    #[test]
    fn line_without_pubtime() {
        let line = r#"{"name":"a","vers":"1"}"#;
        assert_eq!(line_pubtime_secs(line), None);
    }

    #[test]
    fn line_pubtime_realistic() {
        // Compact crates.io-style line with deps before pubtime.
        let line = r#"{"name":"serde","vers":"1.0.1","deps":[{"name":"x","req":"^1"}],"cksum":"ab","features":{},"yanked":false,"pubtime":"2026-03-20T03:13:45Z"}"#;
        assert_eq!(
            line_pubtime_secs(line),
            parse_rfc3339z("2026-03-20T03:13:45Z")
        );
    }

    #[test]
    fn line_pubtime_value_not_key_is_ignored() {
        // A string *value* reading "pubtime" must not be mistaken for the key.
        let line = r#"{"note":"pubtime","pubtime":"2026-03-20T03:13:45Z"}"#;
        assert_eq!(
            line_pubtime_secs(line),
            parse_rfc3339z("2026-03-20T03:13:45Z")
        );
        assert_eq!(line_pubtime_secs(r#"{"note":"pubtime"}"#), None);
    }
}
