//! Cooldown ("age-gating") duration parsing and cutoff math.

#[cfg(test)]
mod tests;

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
