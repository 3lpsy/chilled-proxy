//! Helpers for asserting on the cooldown ETag markers proxies serve.

/// The marker prefix a filtered body carries for a `window`-second cooldown,
/// e.g. `W/"etag123.cd604800-`. The bucket that follows moves with the clock,
/// so tests match the prefix instead of a fixed string.
pub fn marker_prefix(etag: &str, window_secs: u64) -> String {
    let inner = etag.trim_start_matches("W/").trim_matches('"');
    format!("W/\"{inner}.cd{window_secs}-")
}

/// Rewrites a served marker's bucket by `delta`, simulating a client copy that
/// was filtered at an earlier (or later) cutoff bucket.
pub fn shift_marker_bucket(marker: &str, delta: i64) -> String {
    let (head, bucket) = marker
        .rsplit_once('-')
        .expect("marker carries a `-<bucket>` component");
    let digits: String = bucket.chars().take_while(char::is_ascii_digit).collect();
    let tail = &bucket[digits.len()..];
    let shifted = digits
        .parse::<i64>()
        .expect("numeric bucket")
        .saturating_add(delta);
    format!("{head}-{shifted}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matches_a_served_marker() {
        let served = "W/\"etag123.cd604800-480123\"";
        assert!(served.starts_with(&marker_prefix("\"etag123\"", 604_800)));
        assert!(served.starts_with(&marker_prefix("etag123", 604_800)));
    }

    #[test]
    fn bucket_shift_round_trips() {
        let served = "W/\"etag123.cd604800-480123\"";
        assert_eq!(
            shift_marker_bucket(served, -1),
            "W/\"etag123.cd604800-480122\""
        );
        assert_eq!(
            shift_marker_bucket(&shift_marker_bucket(served, -5), 5),
            served
        );
    }

    #[test]
    fn bucket_shift_preserves_a_format_tag() {
        // PyPI markers carry a trailing representation tag after the bucket.
        let served = "W/\"etag123.cd604800-480123.j\"";
        assert_eq!(
            shift_marker_bucket(served, -1),
            "W/\"etag123.cd604800-480122.j\""
        );
    }
}
