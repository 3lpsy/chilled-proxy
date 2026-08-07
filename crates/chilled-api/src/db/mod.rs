//! Sqlite connection, migrations, and the bootstrap admin user.

mod bootstrap;
mod connect;
pub mod entity;
pub mod migration;
#[cfg(test)]
mod tests;

pub use bootstrap::bootstrap_admin;
pub use connect::connect;
