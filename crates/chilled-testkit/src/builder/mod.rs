//! Builder that configures common proxy knobs and starts a registry router
//! in-process against a mock upstream.

mod context;
mod server_builder;

#[cfg(test)]
mod tests;

pub use context::TestContext;
pub use server_builder::TestServerBuilder;
