use super::*;

#[test]
fn duration_units() {
    assert_eq!(parse_duration("3600"), Ok(Duration::from_secs(3600)));
    assert_eq!(parse_duration("3600s"), Ok(Duration::from_secs(3600)));
    assert_eq!(parse_duration("30m"), Ok(Duration::from_secs(1800)));
    assert_eq!(parse_duration("12h"), Ok(Duration::from_secs(43_200)));
    assert_eq!(parse_duration("7d"), Ok(Duration::from_secs(604_800)));
    assert_eq!(parse_duration("1w"), Ok(Duration::from_secs(604_800)));
    assert_eq!(parse_duration("0"), Ok(Duration::from_secs(0)));
    assert_eq!(parse_duration(" 7d "), Ok(Duration::from_secs(604_800)));
}

#[test]
fn duration_rejects() {
    assert!(parse_duration("").is_err());
    assert!(parse_duration("1M").is_err()); // months unsupported
    assert!(parse_duration("1y").is_err()); // years unsupported
    assert!(parse_duration("abc").is_err());
    assert!(parse_duration("d").is_err());
    assert!(parse_duration("7dd").is_err());
}

#[test]
fn duration_overflow() {
    // A bare value at the u64 ceiling parses; multiplying by a unit overflows.
    let max = u64::MAX.to_string();
    assert_eq!(parse_duration(&max), Ok(Duration::from_secs(u64::MAX)));
    let err = parse_duration(&format!("{max}w")).unwrap_err();
    assert!(err.contains("too large"), "unexpected error: {err}");
    // A value that does not even fit in u64 is a parse error, not overflow.
    assert!(parse_duration("99999999999999999999999").is_err());
}

#[test]
fn cutoff_disabled_when_zero() {
    assert_eq!(cutoff_from(1_000_000, Duration::from_secs(0)), None);
    assert_eq!(
        cutoff_from(1_000_000, Duration::from_secs(SECS_PER_DAY)),
        Some(1_000_000 - SECS_PER_DAY)
    );
}

#[test]
fn cutoff_saturates_at_zero() {
    assert_eq!(cutoff_from(10, Duration::from_secs(100)), Some(0));
}
