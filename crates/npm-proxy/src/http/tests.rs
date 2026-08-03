use super::*;

#[test]
fn formats_error_envelope() {
    assert_eq!(format_npm_error("Not found"), r#"{"error":"Not found"}"#);
}

#[test]
fn escapes_error_message() {
    assert_eq!(
        format_npm_error("bad \"quote\"\nnewline"),
        r#"{"error":"bad \"quote\"\nnewline"}"#
    );
}
