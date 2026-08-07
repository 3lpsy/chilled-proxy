//! Live server logs over SSE: backlog + follow, filters, colorization.

#[cfg(test)]
mod tests;

mod line;
mod page;
mod stream;

pub use page::Logs;
