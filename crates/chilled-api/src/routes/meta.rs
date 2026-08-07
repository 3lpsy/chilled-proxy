//! `GET /api/meta` — the bootstrap document the frontend routes off.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chilled_wire::{Meta, MountSummary, UserInfo};
use sea_orm::{EntityTrait, PaginatorTrait};

use crate::authn::MaybeIdentity;
use crate::config::AuthMode;
use crate::db::entity::user;
use crate::state::UiState;

pub(crate) async fn handle_meta(
    State(state): State<UiState>,
    Extension(MaybeIdentity(ident)): Extension<MaybeIdentity>,
) -> Response {
    let needs_first_user = state.config.trust_first_user_signup
        && state.config.auth_mode == AuthMode::Builtin
        && matches!(user::Entity::find().count(&state.db).await, Ok(0));

    let user_info = match &ident {
        Some(ident) => user::Entity::find_by_id(ident.user_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|u| user_info(&u)),
        None => None,
    };

    // Anonymous callers on a private deployment get only what the login page
    // needs — no version, no mount inventory.
    let visible = user_info.is_some() || state.config.public_readonly;
    let mounts = if visible {
        state
            .mounts
            .iter()
            .map(|m| MountSummary {
                name: m.name.clone(),
                kind: m.kind.clone(),
                path: m.path.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };

    Json(Meta {
        version: if visible {
            state.version.clone()
        } else {
            String::new()
        },
        auth_mode: state.config.auth_mode,
        public_readonly: state.config.public_readonly,
        needs_first_user,
        user: user_info,
        login_url: state.config.oidc_login_url.clone(),
        mounts,
    })
    .into_response()
}

/// Projects a user row into its API form.
pub(crate) fn user_info(row: &user::Model) -> UserInfo {
    UserInfo {
        id: row.id,
        username: row.username.clone(),
        auth_source: if row.auth_source == "oidc" {
            AuthMode::Oidc
        } else {
            AuthMode::Builtin
        },
        created_at: row.created_at,
    }
}
