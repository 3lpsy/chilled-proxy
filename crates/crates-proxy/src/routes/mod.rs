//! HTTP request handlers, one module per route.

pub(crate) mod download;
pub(crate) mod index;

pub(crate) use download::handle_download;
pub(crate) use index::handle_index;
