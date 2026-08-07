//! The periodic cache-state snapshot: scan every mount, upsert into sqlite,
//! prune rows the scan no longer sees.

mod retention;
mod run;
mod task;

pub use run::{run_mount, run_once};
pub use task::spawn;
