use super::table::urlencode;

#[test]
fn urlencode_keeps_unreserved_bytes() {
    assert_eq!(urlencode("serde_json-1.0.~x"), "serde_json-1.0.~x");
}

#[test]
fn urlencode_escapes_everything_else() {
    assert_eq!(urlencode("a b&c/d"), "a%20b%26c%2Fd");
    assert_eq!(urlencode("ü"), "%C3%BC");
}
