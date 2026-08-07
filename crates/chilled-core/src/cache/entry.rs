//! Cached upstream response metadata: HTTP validators (etag / mtime) and
//! freshness. Shared by every registry's metadata cache entry.

use std::time::{Duration, Instant, SystemTime};

use crate::http::{fmt_http_date, parse_http_date};

/// HTTP validators and freshness for one cached upstream response. Registries
/// embed this in (or alias it as) their metadata cache entry type.
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
