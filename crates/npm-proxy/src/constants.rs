//! npm-specific compile-time constants.

/// Limit the packument download size to 64 MiB.
pub const DEFAULT_MAX_METADATA_SIZE: usize = 0x400_0000;

/// Limit the tarball download size to 256 MiB.
pub const DEFAULT_MAX_ARTIFACT_SIZE: usize = 0x1000_0000;

/// HTTP Content-Type of served packuments (also our upstream `Accept`).
pub(crate) const PACKUMENT_CTYPE: &str = "application/json";

/// HTTP Content-Type of served tarballs.
pub(crate) const TARBALL_CTYPE: &str = "application/octet-stream";
