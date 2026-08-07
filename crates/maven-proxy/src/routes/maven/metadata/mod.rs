//! Serving `maven-metadata.xml`: the cache/upstream ladder (`serve`), the
//! filter pipeline and generated checksums (`output`), and uncached verbatim
//! forwarding (`passthrough`).

mod output;
mod passthrough;
mod serve;

pub(crate) use passthrough::pass_through;
pub(crate) use serve::{serve_metadata, sidecar_path};
