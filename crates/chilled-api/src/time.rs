//! Unix-seconds clock, matching the cache scanners' `cached_at` convention.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current unix time in whole seconds.
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The current unix time in milliseconds (for log timestamps).
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
