//! Shared application state handed to every request handler.

use crate::model::MavenEntry;
use crate::Config;

/// Shared application state passed to every request handler (cheap to clone).
pub(crate) type AppState = chilled_core::state::AppState<Config, MavenEntry>;
