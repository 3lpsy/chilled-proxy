//! The UI runtime: everything the /api and /ui handlers share.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use sea_orm::DatabaseConnection;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::config::UiConfig;
use crate::logbuf::LogHub;
use crate::mount_view::{MountView, ServerView};

/// A blocking cache scan for one mount, run off the async runtime.
pub type Scanner = Arc<dyn Fn() -> chilled_core::registry::CacheStats + Send + Sync>;

/// Blocking single-artifact deletion; returns re-fetch request paths.
pub type PurgeArtifact = Arc<dyn Fn(&str, &str) -> Vec<String> + Send + Sync>;

/// Blocking whole-mount artifact deletion.
pub type PurgeAll = Arc<dyn Fn() + Send + Sync>;

/// Drives one mount-relative GET through the mount's own router; true = 2xx.
pub type Repull = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>
        + Send
        + Sync,
>;

/// Everything the API can do to one mount's cache.
#[derive(Clone)]
pub struct MountOps {
    pub scan: Scanner,
    pub purge_artifact: PurgeArtifact,
    pub purge_all: PurgeAll,
    pub repull: Repull,
}

impl MountOps {
    /// Scan-only ops with inert deletion/repull — for tests.
    pub fn scan_only(scan: Scanner) -> MountOps {
        MountOps {
            scan,
            purge_artifact: Arc::new(|_, _| Vec::new()),
            purge_all: Arc::new(|| {}),
            repull: Arc::new(|_| Box::pin(async { false })),
        }
    }
}

/// A refresh request: `None` rescans every mount, `Some(name)` just one.
pub type RefreshScope = Option<String>;

/// Built once at startup by [`crate::startup`]; cheap to clone.
#[derive(Clone)]
pub struct UiState(pub(crate) Arc<UiStateInner>);

/// The shared innards; public only because `UiState` derefs to it.
pub struct UiStateInner {
    pub config: UiConfig,
    pub db: DatabaseConnection,
    pub version: String,
    pub server: ServerView,
    pub mounts: Vec<MountView>,
    /// Per-mount cache operations, keyed by mount name.
    pub mounts_ops: Vec<(String, MountOps)>,
    /// Queues on-demand snapshot requests for the background task.
    pub refresh: UnboundedSender<RefreshScope>,
    /// The task side of the queue; taken once by the snapshot loop.
    pub(crate) refresh_rx: Mutex<Option<UnboundedReceiver<RefreshScope>>>,
    /// Usernames already provisioned this process (oidc fast path).
    pub provisioned: Mutex<HashSet<String>>,
    /// The log backlog + live feed the /api/logs stream serves.
    pub log_hub: Arc<LogHub>,
}

impl std::ops::Deref for UiState {
    type Target = UiStateInner;
    fn deref(&self) -> &UiStateInner {
        &self.0
    }
}

impl UiState {
    /// The mount view for a name, if mounted.
    pub(crate) fn mount(&self, name: &str) -> Option<&MountView> {
        self.mounts.iter().find(|m| m.name == name)
    }

    /// The cache operations for a mount name.
    pub(crate) fn ops(&self, name: &str) -> Option<&MountOps> {
        self.mounts_ops
            .iter()
            .find(|(m, _)| m == name)
            .map(|(_, ops)| ops)
    }
}
