//! Shared blackbox-test harness for the chilled-proxy registry proxies.
//!
//! Each test spins up a `wiremock` mock upstream, a temp cache dir, and an
//! in-process registry router served on an ephemeral TCP port, then drives it
//! over real HTTP exactly as the package manager would. Every [`TestServer`]
//! owns its own mock, port, and caches, so tests run fully isolated and concurrent.

pub mod builder;
pub mod marker;
pub mod server;
pub mod time;

pub use builder::{TestContext, TestServerBuilder};
pub use marker::{marker_prefix, shift_marker_bucket};
pub use server::{serve_app, TestServer};
pub use time::{rfc3339_from_now, OLD, TOO_NEW};
