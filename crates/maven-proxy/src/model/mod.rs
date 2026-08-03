//! Metadata response model: HTTP validators (etag / mtime) and freshness.

#[cfg(test)]
mod tests;

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
