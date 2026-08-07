//! PEP 691 age-gating filter and file-URL rewriting.

mod simple;

#[cfg(test)]
mod tests;

pub(crate) use simple::{filter_simple_json, parse_upload_time};
