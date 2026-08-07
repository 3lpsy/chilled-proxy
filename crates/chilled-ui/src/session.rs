//! The bootstrap-session context: one `GET /api/meta` drives the navbar,
//! auth-aware widgets, and the first-user redirect.

use chilled_wire::Meta;
use dioxus::core::spawn_forever;
use dioxus::prelude::*;

use crate::api;

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Loading,
    Ready(Meta),
    Error(String),
}

#[derive(Clone, Copy)]
pub struct SessionCtx {
    pub state: Signal<SessionState>,
}

impl SessionCtx {
    /// The current meta document, if loaded.
    pub fn meta(&self) -> Option<Meta> {
        match &*self.state.read() {
            SessionState::Ready(meta) => Some(meta.clone()),
            _ => None,
        }
    }

    /// Re-fetches /api/meta (after logout, profile edits). `spawn_forever`
    /// survives the caller unmounting, so the navbar never keeps stale state.
    pub fn refresh(&self) {
        let this = *self;
        spawn_forever(async move {
            this.refresh_now().await;
        });
    }

    /// Refreshes and *waits*. Login and setup use this before navigating, so
    /// route guards and the navbar see the new identity, not the stale meta.
    pub async fn refresh_now(&self) {
        let mut state = self.state;
        match api::get_json::<Meta>("/api/meta").await {
            Ok(meta) => state.set(SessionState::Ready(meta)),
            Err(err) => state.set(SessionState::Error(err.to_string())),
        }
    }
}

/// Provides the context and kicks off the initial fetch. Call once, in `app`.
pub fn provide() -> SessionCtx {
    let state = use_signal(|| SessionState::Loading);
    let ctx = use_context_provider(|| SessionCtx { state });
    use_effect(move || ctx.refresh());
    ctx
}

pub fn use_session() -> SessionCtx {
    use_context::<SessionCtx>()
}
