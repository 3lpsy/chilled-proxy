//! Maven-specific compile-time constants.

/// Limit the `maven-metadata.xml` download size to 8 MiB.
pub const DEFAULT_MAX_METADATA_SIZE: usize = 0x80_0000;

/// Limit the artifact file download size to 512 MiB.
pub const DEFAULT_MAX_ARTIFACT_SIZE: usize = 0x2000_0000;

/// Maximum accepted request path length (decoded).
pub(crate) const MAX_PATH_LEN: usize = 1024;

/// Maximum accepted number of path segments.
pub(crate) const MAX_SEGMENTS: usize = 32;

/// Maximum accepted version segment length.
pub(crate) const MAX_VERSION_LEN: usize = 128;

/// The artifact-level (and snapshot version-dir) metadata file name.
pub(crate) const METADATA_FILE: &str = "maven-metadata.xml";

/// HTTP Content-Type for metadata and POM XML.
pub(crate) const XML_CTYPE: &str = "text/xml";

/// HTTP Content-Type for checksums and plain-text errors.
pub(crate) const TEXT_CTYPE: &str = "text/plain; charset=utf-8";

/// HTTP Content-Type for jar files.
pub(crate) const JAR_CTYPE: &str = "application/java-archive";

/// HTTP Content-Type for everything else.
pub(crate) const OCTET_CTYPE: &str = "application/octet-stream";
