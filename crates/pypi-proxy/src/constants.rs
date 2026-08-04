//! PyPI-specific compile-time constants.

/// PEP 691 JSON simple-index content type.
pub(crate) const SIMPLE_JSON_CTYPE: &str = "application/vnd.pypi.simple.v1+json";

/// Version-agnostic alias clients may request instead of the pinned v1 type.
pub(crate) const SIMPLE_JSON_LATEST_CTYPE: &str = "application/vnd.pypi.simple.latest+json";

/// PEP 503 HTML simple-index content type.
pub(crate) const HTML_CTYPE: &str = "text/html; charset=utf-8";

/// PEP 691 HTML simple-index media type, which an upstream may answer with
/// instead of `text/html`.
pub(crate) const SIMPLE_HTML_CTYPE: &str = "application/vnd.pypi.simple.v1+html";

/// Distribution file content type.
pub(crate) const FILE_CTYPE: &str = "application/octet-stream";

/// Plain-text content type (error bodies, refusals).
pub(crate) const TEXT_CTYPE: &str = "text/plain; charset=utf-8";

/// Limit the simple-index download size to 64 MiB.
pub const DEFAULT_MAX_METADATA_SIZE: usize = 0x400_0000;

/// Limit the distribution file download size to 256 MiB.
pub const DEFAULT_MAX_ARTIFACT_SIZE: usize = 0x1000_0000;

/// PEP 658 core-metadata sidecar suffix served alongside a distribution.
pub(crate) const METADATA_SUFFIX: &str = ".metadata";

/// Distribution file extensions accepted on the files route.
pub(crate) const FILE_EXTENSIONS: &[&str] = &[".whl", ".tar.gz", ".zip", ".tar.bz2", ".egg"];
