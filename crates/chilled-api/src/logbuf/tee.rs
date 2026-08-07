//! The `log::Log` tee that feeds the hub.

use std::sync::Arc;

use super::hub::LogHub;

/// A `log::Log` that forwards to env_logger (stdout, formatting unchanged)
/// and tees matching records into the hub.
pub struct TeeLogger {
    inner: env_logger::Logger,
    hub: Arc<LogHub>,
}

impl TeeLogger {
    pub fn new(inner: env_logger::Logger, hub: Arc<LogHub>) -> Self {
        TeeLogger { inner, hub }
    }
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.inner.matches(record) {
            self.hub.push(
                record.level().as_str(),
                record.target(),
                record.args().to_string(),
            );
        }
        self.inner.log(record);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}
