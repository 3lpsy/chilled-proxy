//! In-memory log ring buffer + broadcast, teed off the process logger.

mod hub;
mod tee;
#[cfg(test)]
mod tests;

pub(crate) use hub::level_rank;
pub use hub::LogHub;
pub use tee::TeeLogger;
