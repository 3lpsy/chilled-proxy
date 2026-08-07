//! Version-age probing: `HEAD` the version's POM and read `Last-Modified`.
//!
//! Central never redeploys, so `Last-Modified` is a stable publish time. Any
//! probe failure records first-seen *now* — fail-closed for a full window.

mod pom;

#[cfg(test)]
mod tests;

pub(crate) use pom::{probe_version, probe_versions, Probed};
