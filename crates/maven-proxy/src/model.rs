//! Metadata response model: HTTP validators (etag / mtime) and freshness.

use std::time::{Duration, Instant, SystemTime};

use chilled_core::http::{fmt_http_date, parse_http_date};

/// Cached response metadata for one artifact's `maven-metadata.xml`.
#[derive(Debug, Clone, Default)]
pub(crate) struct MavenEntry {
    /// Upstream HTTP entity tag.
    etag: Option<String>,
    /// Metadata file modification time (from `Last-Modified` or cache mtime).
    mtime: Option<SystemTime>,
    /// Last upstream update-check time.
    atime: Option<Instant>,
}

impl MavenEntry {
    /// Creates an empty entry.
    pub(crate) fn new() -> Self {
        MavenEntry::default()
    }

    /// Whether both entries describe the same upstream content.
    pub(crate) fn is_equivalent(&self, other: &MavenEntry) -> bool {
        (self.etag().is_some() && self.etag() == other.etag())
            || (self.last_modified().is_some() && self.last_modified() == other.last_modified())
    }

    /// Whether the entry is older than the given TTL.
    pub(crate) fn is_expired_with_ttl(&self, ttl: &Duration) -> bool {
        self.atime.is_some_and(|atime| atime.elapsed() > *ttl)
    }

    /// The source-content validator (etag, else last-modified, else empty).
    pub(crate) fn validator(&self) -> String {
        self.etag
            .clone()
            .or_else(|| self.last_modified())
            .unwrap_or_default()
    }

    pub(crate) fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    pub(crate) fn last_modified(&self) -> Option<String> {
        self.mtime.map(fmt_http_date)
    }

    pub(crate) fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    pub(crate) fn set_etag(&mut self, etag: &str) {
        self.etag = Some(etag.to_owned());
    }

    pub(crate) fn set_last_modified(&mut self, last_modified: &str) {
        self.mtime = parse_http_date(last_modified);
    }

    pub(crate) fn set_mtime(&mut self, mtime: SystemTime) {
        self.mtime = Some(mtime);
    }

    /// Records that upstream was consulted just now.
    pub(crate) fn set_last_updated(&mut self) {
        self.atime = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::MavenEntry;

    #[test]
    fn equivalence_prefers_etag() {
        let mut a = MavenEntry::new();
        let mut b = MavenEntry::new();
        a.set_etag("\"x\"");
        b.set_etag("\"x\"");
        assert!(a.is_equivalent(&b));
        b.set_etag("\"y\"");
        assert!(!a.is_equivalent(&b));
    }

    #[test]
    fn equivalence_falls_back_to_last_modified() {
        let mut a = MavenEntry::new();
        let mut b = MavenEntry::new();
        a.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
        b.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
        assert!(a.is_equivalent(&b));
        assert!(!a.is_equivalent(&MavenEntry::new()));
    }

    #[test]
    fn empty_entries_are_never_equivalent() {
        assert!(!MavenEntry::new().is_equivalent(&MavenEntry::new()));
    }

    #[test]
    fn validator_prefers_etag_over_last_modified() {
        let mut e = MavenEntry::new();
        assert_eq!(e.validator(), "");
        e.set_last_modified("Sat, 01 Jan 2000 00:00:00 GMT");
        assert_eq!(e.validator(), "Sat, 01 Jan 2000 00:00:00 GMT");
        e.set_etag("\"x\"");
        assert_eq!(e.validator(), "\"x\"");
    }

    #[test]
    fn expiry_requires_a_recorded_update() {
        let mut e = MavenEntry::new();
        assert!(!e.is_expired_with_ttl(&Duration::ZERO));
        e.set_last_updated();
        assert!(e.is_expired_with_ttl(&Duration::ZERO));
        assert!(!e.is_expired_with_ttl(&Duration::from_secs(3600)));
    }

    #[test]
    fn last_modified_round_trips_mtime() {
        let mut e = MavenEntry::new();
        e.set_mtime(SystemTime::UNIX_EPOCH);
        assert_eq!(e.last_modified().unwrap(), "Thu, 01 Jan 1970 00:00:00 GMT");
    }
}
