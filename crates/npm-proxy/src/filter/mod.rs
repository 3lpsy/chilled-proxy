//! Packument age-gating filter and tarball URL rewriting (serde_json).

mod packument;

#[cfg(test)]
mod tests;

pub(crate) use packument::{filter_bytes, FilterResult};
