//! Cache primitives shared by every registry proxy: response metadata entries
//! ([`entry`]), on-disk file store/fetch ([`fs`]), and bounded in-memory
//! metadata and filtered-body caches ([`metadata`], [`memo`]).

#[cfg(test)]
mod tests;

pub mod entry;
pub mod fs;
pub mod memo;
pub mod metadata;

pub use entry::CacheEntry;
pub use memo::{FilteredMemo, MEMO_BUCKET_SECS};
pub use metadata::MetadataCache;
