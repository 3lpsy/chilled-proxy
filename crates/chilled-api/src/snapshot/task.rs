//! The background snapshot task loop.

use log::{info, warn};
use tokio::time::MissedTickBehavior;

use super::run::run_scoped;
use crate::state::UiState;

/// Runs snapshots forever: every interval tick (all mounts), plus queued
/// on-demand requests (all or one mount). All runs serialize through this
/// loop, so scoped and full passes never interleave writes. The first tick
/// fires immediately so the UI has data.
pub fn spawn(state: UiState) -> tokio::task::JoinHandle<()> {
    let mut rx = state
        .refresh_rx
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .expect("snapshot task spawned once");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(state.config.cache_update_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let scope = tokio::select! {
                _ = interval.tick() => None,
                req = rx.recv() => match req {
                    Some(scope) => scope,
                    // All senders gone: the state is being torn down.
                    None => break,
                },
            };
            match run_scoped(&state, scope.as_deref()).await {
                Ok(count) => match &scope {
                    Some(mount) => info!("ui: snapshot of '{mount}' complete, {count} artifacts"),
                    None => info!("ui: snapshot complete, {count} artifacts"),
                },
                // A failed scan must not kill the task; the next tick retries.
                Err(err) => warn!("ui: snapshot failed: {err}"),
            }
        }
    })
}
