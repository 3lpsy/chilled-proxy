//! The caller's own profile: `/api/users/me`.

use axum::extract::State;
use axum::http::header::COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chilled_wire::UpdateProfileReq;
use sea_orm::{ActiveValue, ColumnTrait, EntityTrait, QueryFilter};

use crate::authn::{password, require, session, MaybeIdentity};
use crate::config::AuthMode;
use crate::db::entity::{session as session_row, user};
use crate::routes::api_error;
use crate::routes::meta::user_info;
use crate::routes::setup::MIN_PASSWORD_LEN;
use crate::state::UiState;
use crate::time::now;

/// `GET /api/users/me`
pub(crate) async fn handle_me(
    State(state): State<UiState>,
    Extension(MaybeIdentity(ident)): Extension<MaybeIdentity>,
) -> Response {
    let ident = match require(&ident) {
        Ok(ident) => ident,
        Err(res) => return *res,
    };
    match user::Entity::find_by_id(ident.user_id).one(&state.db).await {
        Ok(Some(row)) => Json(user_info(&row)).into_response(),
        _ => api_error(StatusCode::NOT_FOUND, "no such user"),
    }
}

/// `PATCH /api/users/me` — username/password change, builtin accounts only.
pub(crate) async fn handle_update_me(
    State(state): State<UiState>,
    Extension(MaybeIdentity(ident)): Extension<MaybeIdentity>,
    headers: HeaderMap,
    Json(req): Json<UpdateProfileReq>,
) -> Response {
    let ident = match require(&ident) {
        Ok(ident) => ident.clone(),
        Err(res) => return *res,
    };
    if ident.auth_source == AuthMode::Oidc {
        return api_error(
            StatusCode::CONFLICT,
            "profile is managed by the identity provider",
        );
    }
    let Ok(Some(row)) = user::Entity::find_by_id(ident.user_id).one(&state.db).await else {
        return api_error(StatusCode::NOT_FOUND, "no such user");
    };
    let verified = row
        .password_hash
        .as_deref()
        .is_some_and(|phc| password::verify(&req.current_password, phc));
    if !verified {
        return api_error(StatusCode::UNAUTHORIZED, "current password is wrong");
    }

    let mut change: user::ActiveModel = row.into();
    if let Some(username) = req.username.as_deref().map(str::trim) {
        if username.is_empty() || username.len() > 64 {
            return api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "username must be 1-64 characters",
            );
        }
        change.username = ActiveValue::Set(username.to_owned());
    }
    let password_changed = match req.new_password.as_deref() {
        Some(pw) => {
            if pw.len() < MIN_PASSWORD_LEN {
                return api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "password must be at least 8 characters",
                );
            }
            match password::hash(pw) {
                Ok(hash) => change.password_hash = ActiveValue::Set(Some(hash)),
                Err(err) => {
                    log::error!("ui: {err}");
                    return api_error(StatusCode::INTERNAL_SERVER_ERROR, "hashing failed");
                }
            }
            true
        }
        None => false,
    };
    change.updated_at = ActiveValue::Set(now());
    let updated = match user::Entity::update(change).exec(&state.db).await {
        Ok(updated) => updated,
        Err(_) => return api_error(StatusCode::CONFLICT, "username already exists"),
    };

    // A password change signs out every *other* session of this user; the
    // session that made the change stays valid.
    if password_changed {
        let current: Vec<String> = headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|h| h.to_str().ok())
            .flat_map(session::cookie_values)
            .map(session::token_hash)
            .collect();
        let mut purge = session_row::Entity::delete_many()
            .filter(session_row::Column::UserId.eq(ident.user_id));
        if !current.is_empty() {
            purge = purge.filter(session_row::Column::TokenHash.is_not_in(current));
        }
        let _ = purge.exec(&state.db).await;
    }
    Json(user_info(&updated)).into_response()
}
