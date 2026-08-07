//! Embedded schema migrations, applied at startup.

mod m0001_initial;
mod migrator;

pub use migrator::Migrator;
