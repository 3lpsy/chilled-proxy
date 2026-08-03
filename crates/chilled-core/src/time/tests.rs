use super::*;

#[test]
fn rfc3339_epoch() {
    assert_eq!(parse_rfc3339z("1970-01-01T00:00:00Z"), Some(0));
}

#[test]
fn rfc3339_sample() {
    let got = parse_rfc3339z("2026-03-20T03:13:45Z").unwrap();
    // 2026-03-20 is day 20532 since 1970-01-01.
    assert_eq!(got, 20532 * 86_400 + 3 * 3600 + 13 * 60 + 45);
}

#[test]
fn rfc3339_fractional() {
    assert_eq!(
        parse_rfc3339z("2026-03-20T03:13:45.999Z"),
        parse_rfc3339z("2026-03-20T03:13:45Z"),
    );
}

#[test]
fn rfc3339_rejects_malformed() {
    assert_eq!(parse_rfc3339z(""), None);
    assert_eq!(parse_rfc3339z("2026-03-20T03:13:45"), None); // no Z
    assert_eq!(parse_rfc3339z("2026-03-20 03:13:45Z"), None); // no T
    assert_eq!(parse_rfc3339z("2026-13-01T00:00:00Z"), None); // bad month
    assert_eq!(parse_rfc3339z("2026-01-32T00:00:00Z"), None); // bad day
    assert_eq!(parse_rfc3339z("2026-01-01T24:00:00Z"), None); // bad hour
    assert_eq!(parse_rfc3339z("1969-12-31T23:59:59Z"), None); // pre-epoch
}
