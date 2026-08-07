//! Identity resolution middleware and the per-tier guards.

use axum::extract::{Request, State};
use axum::http::header::COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use log::warn;
use sea_orm::{ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use super::session;
use crate::config::AuthMode;
use crate::db::entity::{session as session_row, user};
use crate::routes::api_error;
use crate::state::UiState;
use crate::time::now;

/// The authenticated caller, attached to every request as an extension.
#[derive(Debug, Clone)]
pub(crate) struct Identity {
    pub user_id: i64,
    pub auth_source: AuthMode,
}

/// Present on every request once the identity middleware ran.
#[derive(Debug, Clone)]
pub(crate) struct MaybeIdentity(pub Option<Identity>);

/// Resolves the caller's identity and attaches it. Applied with `.layer()` on
/// the whole UI router so fallbacks are covered too.
pub(crate) async fn identity(
    State(state): State<UiState>,
    mut req: Request,
    next: Next,
) -> Response {
    let ident = resolve(&state, req.headers()).await;
    req.extensions_mut().insert(MaybeIdentity(ident));
    next.run(req).await
}

/// Guard for the mutating tier (and user/log reads): authenticated or 401.
pub(crate) async fn require_auth(req: Request, next: Next) -> Response {
    if is_authenticated(&req) {
        next.run(req).await
    } else {
        api_error(StatusCode::UNAUTHORIZED, "authentication required")
    }
}

/// Guard for the readonly tier: authenticated, or anyone when
/// `--ui-public-readonly-enabled` is on.
pub(crate) async fn require_read(
    State(state): State<UiState>,
    req: Request,
    next: Next,
) -> Response {
    if state.config.public_readonly || is_authenticated(&req) {
        next.run(req).await
    } else {
        api_error(StatusCode::UNAUTHORIZED, "authentication required")
    }
}

/// The guard guarantees an identity on its tier; a missing one is a bug.
pub(crate) fn require(ident: &Option<Identity>) -> Result<&Identity, Box<Response>> {
    ident.as_ref().ok_or_else(|| {
        Box::new(api_error(
            StatusCode::UNAUTHORIZED,
            "authentication required",
        ))
    })
}

fn is_authenticated(req: &Request) -> bool {
    req.extensions()
        .get::<MaybeIdentity>()
        .is_some_and(|m| m.0.is_some())
}

async fn resolve(state: &UiState, headers: &HeaderMap) -> Option<Identity> {
    match state.config.auth_mode {
        AuthMode::Oidc => resolve_oidc(state, headers).await,
        AuthMode::Builtin => resolve_session(state, headers).await,
    }
}

/// oidc mode: trust the configured header, provisioning the user on first
/// sight. The provisioned set only skips the insert attempt, not the lookup.
async fn resolve_oidc(state: &UiState, headers: &HeaderMap) -> Option<Identity> {
    let header = state.config.oidc_user_header.as_deref()?;
    let username = headers.get(header)?.to_str().ok()?.trim();
    if username.is_empty() {
        return None;
    }
    let known = state
        .provisioned
        .lock()
        .is_ok_and(|set| set.contains(username));
    if !known {
        provision_oidc_user(&state.db, username).await;
        if let Ok(mut set) = state.provisioned.lock() {
            set.insert(username.to_owned());
        }
    }
    let found = user::Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(&state.db)
        .await
        .ok()??;
    Some(Identity {
        user_id: found.id,
        auth_source: AuthMode::Oidc,
    })
}

/// Creates the oidc user if missing; a lost unique-insert race is fine.
async fn provision_oidc_user(db: &DatabaseConnection, username: &str) {
    let exists = user::Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(db)
        .await;
    if !matches!(exists, Ok(None)) {
        return;
    }
    let ts = now();
    let row = user::ActiveModel {
        username: ActiveValue::Set(username.to_owned()),
        auth_source: ActiveValue::Set("oidc".to_owned()),
        password_hash: ActiveValue::Set(None),
        created_at: ActiveValue::Set(ts),
        updated_at: ActiveValue::Set(ts),
        ..Default::default()
    };
    if let Err(err) = user::Entity::insert(row).exec(db).await {
        // Unique-violation from a concurrent request is expected; the re-find
        // in the caller settles it either way.
        warn!("ui: oidc user provisioning for '{username}': {err}");
    }
}

/// builtin mode: check every session cookie against unexpired session rows.
async fn resolve_session(state: &UiState, headers: &HeaderMap) -> Option<Identity> {
    for header in headers.get_all(COOKIE) {
        let Ok(raw) = header.to_str() else { continue };
        for token in session::cookie_values(raw) {
            let hash = session::token_hash(token);
            let found = session_row::Entity::find()
                .filter(session_row::Column::TokenHash.eq(hash))
                .filter(session_row::Column::ExpiresAt.gt(now()))
                .find_also_related(user::Entity)
                .one(&state.db)
                .await;
            if let Ok(Some((_, Some(owner)))) = found {
                return Some(Identity {
                    user_id: owner.id,
                    auth_source: AuthMode::Builtin,
                });
            }
        }
    }
    None
}
