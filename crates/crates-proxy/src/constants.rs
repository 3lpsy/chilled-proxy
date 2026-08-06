//! crates.io-specific compile-time constants.

/// Upstream `crates.io` registry index URL.
pub const INDEX_CRATES_IO_URL: &str = "https://index.crates.io/";

/// Upstream `crates.io` registry URL.
pub const CRATES_IO_URL: &str = "https://crates.io/";

/// Crates download API path, relative so it appends to an upstream URL that
/// carries a path prefix (e.g. a Nexus repository under `/repository/cargo/`)
/// instead of replacing it. Also joined onto this proxy's mount URL for
/// `config.json`.
pub(crate) const CRATES_API_PATH: &str = "api/v1/crates/";

/// Crates download API path relative to this proxy's mount (for `config.json`).
pub(crate) const CRATES_API_REL: &str = "api/v1/crates";

/// Limit the crate file download size to 16 MiB.
pub const DEFAULT_MAX_ARTIFACT_SIZE: usize = 0x100_0000;

/// Limit the sparse-index entry download size to 64 MiB.
pub const DEFAULT_MAX_METADATA_SIZE: usize = 0x400_0000;

/// HTTP Content-Type of the registry index entry JSON file.
pub(crate) const INDEX_CTYPE: &str = "text/plain";

/// HTTP Content-Type of the crate package file.
pub(crate) const CRATE_CTYPE: &str = "application/x-tar";
