//! Cached upstream response metadata: HTTP validators (etag / mtime) and
//! freshness. Shared by every registry's metadata cache entry.

use std::time::{Duration, Instant, SystemTime};

use crate::http::{fmt_http_date, parse_http_date};

/// HTTP validators and freshness for one cached upstream response.
///
/// Registries embed this in (or alias it as) their metadata cache entry type.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CacheEntry {
    /// Upstream HTTP entity tag.
    etag: Option<String>,
    /// Body modification time (from `Last-Modified` or the cache file mtime).
    mtime: Option<SystemTime>,
    /// Last upstream update-check time.
    atime: Option<Instant>,
}

impl CacheEntry {
    /// Creates an empty entry.
    #[must_use]
    pub fn new() -> Self {
        CacheEntry::default()
    }

    /// Whether both entries describe the same upstream content.
    #[must_use]
    pub fn is_equivalent(&self, other: &CacheEntry) -> bool {
        (self.etag().is_some() && (self.etag() == other.etag()))
            || (self.last_modified().is_some() && (self.last_modified() == other.last_modified()))
    }

    /// Whether the entry is older than the given TTL.
    #[must_use]
    pub fn is_expired_with_ttl(&self, ttl: &Duration) -> bool {
        self.atime.is_some_and(|atime| atime.elapsed() > *ttl)
    }

    /// The source-content validator (etag, else last-modified, else empty),
    /// used as a memo key and as the base of the weak marked ETag.
    #[must_use]
    pub fn validator(&self) -> String {
        self.etag
            .clone()
            .or_else(|| self.last_modified())
            .unwrap_or_default()
    }

    /// Gets the HTTP entity tag metadata.
    #[must_use]
    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// Gets the HTTP `Last-Modified` metadata.
    #[must_use]
    pub fn last_modified(&self) -> Option<String> {
        self.mtime.map(fmt_http_date)
    }

    /// Gets the body modification time metadata.
    #[must_use]
    pub fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    /// Sets the HTTP entity tag metadata.
    pub fn set_etag(&mut self, etag: &str) {
        self.etag = Some(etag.to_owned());
    }

    /// Sets the HTTP `Last-Modified` metadata.
    pub fn set_last_modified(&mut self, last_modified: &str) {
        self.mtime = parse_http_date(last_modified);
    }

    /// Sets the body modification time metadata.
    pub fn set_mtime(&mut self, mtime: SystemTime) {
        self.mtime = Some(mtime);
    }

    /// Records that upstream was consulted just now.
    pub fn set_last_updated(&mut self) {
        self.atime = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn equivalence_prefers_etag_then_last_modified() {
        let mut a = CacheEntry::new();
        let mut b = CacheEntry::new();
        assert!(!a.is_equivalent(&b));

        a.set_etag("\"e1\"");
        b.set_etag("\"e1\"");
        assert!(a.is_equivalent(&b));
        b.set_etag("\"e2\"");
        assert!(!a.is_equivalent(&b));

        let mut c = CacheEntry::new();
        let mut d = CacheEntry::new();
        c.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        d.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        assert!(c.is_equivalent(&d));
        assert!(!c.is_equivalent(&CacheEntry::new()));
    }

    #[test]
    fn last_modified_round_trips() {
        let mut e = CacheEntry::new();
        e.set_last_modified("Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(
            e.last_modified().as_deref(),
            Some("Sun, 06 Nov 1994 08:49:37 GMT")
        );

        let mut m = CacheEntry::new();
        m.set_mtime(UNIX_EPOCH);
        assert_eq!(
            m.last_modified().as_deref(),
            Some("Thu, 01 Jan 1970 00:00:00 GMT")
        );
    }

    #[test]
    fn validator_prefers_etag_over_last_modified() {
        let mut e = CacheEntry::new();
        assert_eq!(e.validator(), "");
        e.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
        assert_eq!(e.validator(), "Sat, 01 Jan 2000 00:00:00 GMT");
        e.set_etag("\"x\"");
        assert_eq!(e.validator(), "\"x\"");
    }

    #[test]
    fn expiry_requires_a_recorded_update() {
        let mut e = CacheEntry::new();
        assert!(!e.is_expired_with_ttl(&Duration::ZERO));
        e.set_last_updated();
        assert!(!e.is_expired_with_ttl(&Duration::from_secs(3600)));
        std::thread::sleep(Duration::from_millis(2));
        assert!(e.is_expired_with_ttl(&Duration::ZERO));
    }
}
