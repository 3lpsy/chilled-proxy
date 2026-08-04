//! Cooldown ("age-gating") duration parsing and cutoff math.

use std::time::Duration;

/// Number of seconds in one calendar day.
pub const SECS_PER_DAY: u64 = 86_400;

/// Compute the cutoff (unix seconds) for a cooldown window measured back from
/// `now_secs`. A zero window means "no filtering" and yields `None`.
pub fn cutoff_from(now_secs: u64, cooldown: Duration) -> Option<u64> {
    let secs = cooldown.as_secs();
    if secs == 0 {
        None
    } else {
        Some(now_secs.saturating_sub(secs))
    }
}

/// Parse a cooldown duration: a bare integer (seconds) or an integer with a
/// single unit suffix (`s`, `m`, `h`, `d`, `w`). Months/years are unsupported
/// so `m` stays unambiguously minutes.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".to_string());
    }

    let last = s.as_bytes()[s.len() - 1];
    let (digits, mult) = if last.is_ascii_digit() {
        (s, 1)
    } else {
        let mult = match last {
            b's' => 1,
            b'm' => 60,
            b'h' => 3_600,
            b'd' => SECS_PER_DAY,
            b'w' => 7 * SECS_PER_DAY,
            _ => {
                return Err(format!(
                    "invalid duration unit '{}' in '{s}' (use s, m, h, d, or w)",
                    last as char
                ));
            }
        };
        (&s[..s.len() - 1], mult)
    };

    let value: u64 = digits
        .parse()
        .map_err(|_| format!("invalid duration value in '{s}'"))?;

    value
        .checked_mul(mult)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("duration '{s}' is too large"))
}

#[cfg(test)]
mod tests {
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
}
