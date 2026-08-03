//! Cache primitives shared by every registry proxy.
//!
//! - [`fs`] — on-disk file store/fetch with optional mtime control.
//! - [`metadata`] — bounded in-memory metadata cache (etag / mtime entries).
//! - [`memo`] — bounded in-memory memo of filtered response bodies.

pub mod fs;
pub mod memo;
pub mod metadata;

pub use memo::{FilteredMemo, MEMO_BUCKET_SECS};
pub use metadata::MetadataCache;
