//! Memoization of filtered/rewritten response bodies.
//!
//! Filtering is CPU work repeated on every served request. The memo caches the
//! produced bytes keyed by `(key, source-validator, cutoff bucket)`; a mismatch
//! on either tag guarantees stale or time-shifted output is never returned.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::RwLock;

use bytes::Bytes;
use log::debug;

/// Granularity (seconds) of the memo cutoff bucket. The filtered output only
/// changes when a version crosses the cutoff; hour-bucketing keeps the key
/// stable at the cost of ≤1h aging-in jitter — irrelevant for day-scale cooldowns.
pub const MEMO_BUCKET_SECS: u64 = 3600;

/// Maximum keys held before the memo is cleared (bounds memory use).
const MEMO_MAX_ENTRIES: usize = 8192;

/// One memoized body, tagged with its source identity and cutoff bucket.
struct MemoEntry {
    /// Source content validator (upstream etag or last-modified).
    validator: String,
    /// Cutoff bucket (`cutoff / MEMO_BUCKET_SECS`) the body was produced for.
    bucket: u64,
    /// The produced bytes (cheap to clone).
    data: Bytes,
}

/// Bounded, concurrent cache of filtered bodies keyed by package (and, when a
/// registry serves multiple representations, a caller-chosen key suffix).
pub struct FilteredMemo {
    inner: RwLock<HashMap<String, MemoEntry>>,
}

impl Default for FilteredMemo {
    fn default() -> Self {
        Self::new()
    }
}

impl FilteredMemo {
    /// Creates an empty memo.
    pub fn new() -> Self {
        FilteredMemo {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the memoized body for `key` if it was produced from the same
    /// source `validator` and cutoff `bucket`.
    ///
    /// An empty validator means the upstream offered neither an ETag nor a
    /// `Last-Modified`, so nothing distinguishes one body from the next — the
    /// memo declines rather than risk serving superseded bytes.
    pub fn get(&self, key: &str, validator: &str, bucket: u64) -> Option<Bytes> {
        if validator.is_empty() {
            return None;
        }
        let map = self.inner.read().unwrap();
        let entry = map.get(key)?;
        (entry.validator == validator && entry.bucket == bucket).then(|| entry.data.clone())
    }

    /// Stores the body for `key`, evicting everything if the memo is full and
    /// this is a new key (keeps memory bounded). Bodies with no source
    /// validator are not memoized (see [`Self::get`]).
    pub fn put(&self, key: String, validator: String, bucket: u64, data: Bytes) {
        if validator.is_empty() {
            return;
        }
        let mut map = self.inner.write().unwrap();
        if map.len() >= MEMO_MAX_ENTRIES && !map.contains_key(&key) {
            debug!("memo: cleared filtered-body memo at capacity");
            map.clear();
        }
        map.insert(
            key,
            MemoEntry {
                validator,
                bucket,
                data,
            },
        );
    }
}
