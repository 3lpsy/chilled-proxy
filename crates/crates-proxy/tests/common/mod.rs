//! crates-proxy blackbox harness: a thin, crates-flavored layer over
//! `chilled_testkit` (mock upstream, temp cache, in-process router on an
//! ephemeral port). Each test binary uses only a subset of these re-exports.
#![allow(unused_imports, dead_code)]

pub mod fixtures;
pub mod proxy;

pub use fixtures::*;
pub use proxy::{TestProxy, TestProxyBuilder};
