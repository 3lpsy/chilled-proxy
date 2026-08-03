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
