//! Timestamp parsing shared by the registry metadata filters.

use crate::cooldown::SECS_PER_DAY;

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff]Z` into unix seconds. Fractional seconds are truncated.
pub fn parse_rfc3339z(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut dp = date.split('-');
    let y: i32 = dp.next()?.parse().ok()?;
    let mo: u32 = dp.next()?.parse().ok()?;
    let d: u32 = dp.next()?.parse().ok()?;
    if dp.next().is_some() {
        return None;
    }
    let mut tp = time.split(':');
    let h: u64 = tp.next()?.parse().ok()?;
    let mi: u64 = tp.next()?.parse().ok()?;
    let sec: u64 = tp.next()?.split('.').next()?.parse().ok()?;
    if tp.next().is_some() || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let days = days_since_epoch(y, mo, d)?;
    if days < 0 {
        return None;
    }
    Some(days as u64 * SECS_PER_DAY + h * 3_600 + mi * 60 + sec)
}

/// Civil UTC date → days since 1970-01-01. Based on Howard Hinnant's `days_from_civil`.
fn days_since_epoch(y: i32, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let m = m as i64;
    let d = d as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y as i64 - era as i64 * 400;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era as i64 * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
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
}
