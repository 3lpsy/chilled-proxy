//! Management API, sqlite persistence, and embedded web UI for chilled-proxy.
//! The binary pre-validates config and redacts secrets before they reach here.

pub(crate) mod assets;
pub(crate) mod authn;
pub mod config;
pub mod db;
pub mod logbuf;
pub mod mount_view;
pub(crate) mod routes;
pub mod snapshot;
pub mod state;
pub(crate) mod time;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use axum::middleware::from_fn_with_state;
use axum::Router;

pub use config::{AuthMode, UiConfig};
pub use logbuf::{LogHub, TeeLogger};
pub use mount_view::{MountView, ServerView};
pub use snapshot::spawn as spawn_snapshot_task;
pub use state::{MountOps, Scanner, UiState};

/// Connects the database, migrates it, and creates the bootstrap user; any
/// failure stops startup. A `None` log_hub gets a fresh empty hub (for tests).
pub async fn startup(
    config: UiConfig,
    version: String,
    server: ServerView,
    mounts: Vec<MountView>,
    mounts_ops: Vec<(String, MountOps)>,
    log_hub: Option<Arc<LogHub>>,
) -> Result<UiState, String> {
    let db = db::connect(&config.db_path).await?;
    if let (Some(username), Some(pw)) = (&config.admin_username, &config.admin_password) {
        db::bootstrap_admin(&db, username, pw).await?;
    }
    let (refresh, refresh_rx) = tokio::sync::mpsc::unbounded_channel();
    Ok(UiState(Arc::new(state::UiStateInner {
        config,
        db,
        version,
        server,
        mounts,
        mounts_ops,
        refresh,
        refresh_rx: Mutex::new(Some(refresh_rx)),
        provisioned: Mutex::new(HashSet::new()),
        log_hub: log_hub.unwrap_or_default(),
    })))
}

/// The complete /api + /ui router. Identity resolution wraps only /api; the
/// unauthenticated /ui assets never cost a session DB query per static file.
pub fn ui_router(state: UiState) -> Router {
    routes::api_router(&state)
        .layer(from_fn_with_state(state.clone(), authn::identity))
        .merge(assets::router())
        .with_state(state)
}
