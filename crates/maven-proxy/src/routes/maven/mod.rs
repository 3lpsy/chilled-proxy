//! The single Maven wildcard handler: classification, the metadata serve
//! ladder, generated checksums, and artifact downloads.
//!
//! Split by role: path classification (`route`), the axum entry point
//! (`handler`), `maven-metadata.xml` serving and filtering (`metadata`), and
//! artifact downloads (`artifact`).

mod artifact;
mod handler;
mod metadata;
mod route;

pub(crate) use handler::handle_maven;
