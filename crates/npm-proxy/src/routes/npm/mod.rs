//! The npm request handler: packuments, version docs, and tarball downloads.
//!
//! Registered as the router fallback so the raw URI path is classified with
//! exactly one percent-decode — axum's `Path` extractor would decode `%2f` and
//! make `/@scope%2fname` indistinguishable from `/@scope/name`.
//!
//! Split by role: classification (`route`), the axum entry point (`handler`),
//! packument and version-doc serving (`packument`), tarballs (`tarball`), and
//! the on-disk cache helpers both use (`cache`).

mod cache;
mod handler;
mod packument;
mod route;
mod tarball;

pub(crate) use handler::handle_npm;
