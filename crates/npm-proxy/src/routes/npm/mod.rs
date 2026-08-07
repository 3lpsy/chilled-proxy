//! The npm request handler: packuments, version docs, and tarball downloads.
//!
//! Registered as the router fallback so the raw URI path is classified with
//! exactly one percent-decode — axum's `Path` extractor would decode `%2f` and
//! make `/@scope%2fname` indistinguishable from `/@scope/name`.

mod cache;
mod fetch;
mod handler;
mod packument;
mod route;
mod tarball;
mod version_doc;

pub(crate) use handler::handle_npm;
