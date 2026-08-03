use super::*;
use axum::http::header;

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
