use super::*;

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
