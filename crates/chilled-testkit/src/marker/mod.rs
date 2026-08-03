//! Helpers for asserting on the cooldown ETag markers proxies serve.

#[cfg(test)]
mod tests;

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
