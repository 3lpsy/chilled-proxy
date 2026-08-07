//! `GET /index/<path>` — proxied, cached, age-gated sparse-index entries.
//!
//! Split by role: the axum entry point (`handler`), the cache/upstream serve
//! ladder (`fetch`), and response building, filtering, and the cache (`serve`).

mod fetch;
mod handler;
mod serve;

pub(crate) use fetch::download_index_entry;
pub(crate) use handler::handle_index;
pub(crate) use serve::cache_write_index;
