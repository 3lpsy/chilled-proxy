//! Metadata response model: HTTP validators (etag / mtime) and freshness.

/// Cached response metadata for one artifact's `maven-metadata.xml`.
pub(crate) type MavenEntry = chilled_core::cache::CacheEntry;
