use super::*;

#[test]
fn error_body_is_well_formed() {
    let body = format_json_error("bad \"quote\" and \\slash");
    assert_eq!(
        body,
        r#"{"errors":[{"detail":"bad \"quote\" and \\slash"}]}"#
    );
}
