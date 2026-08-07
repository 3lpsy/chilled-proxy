use std::time::UNIX_EPOCH;

use axum::http::header;
use bytes::Bytes;

use super::response::JSON_CTYPE;
use super::{
    data_response, error_response, fmt_http_date, json_escape, json_response, parse_http_date,
    text_response, FetchError,
};

// --- fetch ---

#[test]
fn fetch_error_displays() {
    assert_eq!(FetchError::TooLarge.to_string(), "response body too large");
}

// --- httpdate ---

#[test]
fn rfc_example_round_trip() {
    let s = "Sun, 06 Nov 1994 08:49:37 GMT";
    let t = parse_http_date(s).unwrap();
    // 784111777 is the canonical unix time for this date.
    assert_eq!(t.duration_since(UNIX_EPOCH).unwrap().as_secs(), 784_111_777);
    assert_eq!(fmt_http_date(t), s);
}

#[test]
fn epoch() {
    let t = UNIX_EPOCH;
    assert_eq!(fmt_http_date(t), "Thu, 01 Jan 1970 00:00:00 GMT");
    assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(t));
}

#[test]
fn rejects_garbage() {
    assert_eq!(parse_http_date(""), None);
    assert_eq!(parse_http_date("not a date"), None);
    assert_eq!(parse_http_date("Sun, 06 Xxx 1994 08:49:37 GMT"), None);
    assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 PST"), None);
}

// --- response ---

#[test]
fn escapes_json_metacharacters() {
    assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    assert_eq!(json_escape("line\nbreak\ttab"), "line\\nbreak\\ttab");
    assert_eq!(json_escape("ctrl\u{0001}"), "ctrl\\u0001");
    // Non-ASCII passes through unchanged (valid UTF-8 in a JSON string).
    assert_eq!(json_escape("café"), "café");
}

#[test]
fn builders_set_status_and_content_type() {
    assert_eq!(error_response(404).status(), 404);

    let r = json_response(200, "{}".into());
    assert_eq!(r.headers()[header::CONTENT_TYPE], JSON_CTYPE);

    let r = text_response(200, "text/xml", "<a/>".into());
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/xml");

    let r = data_response("application/x-tar", Bytes::from_static(b"x"));
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()[header::CONTENT_TYPE], "application/x-tar");
}
