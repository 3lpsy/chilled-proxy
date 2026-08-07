//! Listing, creating, and deleting users.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chilled_wire::CreateUserReq;
use sea_orm::{ActiveValue, EntityTrait, QueryOrder};

use crate::authn::{password, require, MaybeIdentity};
use crate::config::AuthMode;
use crate::db::entity::user;
use crate::routes::api_error;
use crate::routes::meta::user_info;
use crate::routes::setup::check_new_credentials;
use crate::state::UiState;
use crate::time::now;

/// `GET /api/users`
pub(crate) async fn handle_list(State(state): State<UiState>) -> Response {
    match user::Entity::find()
        .order_by_asc(user::Column::Username)
        .all(&state.db)
        .await
    {
        Ok(users) => Json(users.iter().map(user_info).collect::<Vec<_>>()).into_response(),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "database error"),
    }
}

/// `POST /api/users` — builtin accounts only.
pub(crate) async fn handle_create(
    State(state): State<UiState>,
    Json(req): Json<CreateUserReq>,
) -> Response {
    if state.config.auth_mode == AuthMode::Oidc {
        return api_error(
            StatusCode::CONFLICT,
            "users are managed by the identity provider",
        );
    }
    let username = req.username.trim().to_owned();
    if let Err(msg) = check_new_credentials(&username, &req.password) {
        return api_error(StatusCode::UNPROCESSABLE_ENTITY, msg);
    }
    let hash = match password::hash(&req.password) {
        Ok(hash) => hash,
        Err(err) => {
            log::error!("ui: {err}");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "hashing failed");
        }
    };
    let ts = now();
    let inserted = user::Entity::insert(user::ActiveModel {
        username: ActiveValue::Set(username),
        auth_source: ActiveValue::Set("builtin".to_owned()),
        password_hash: ActiveValue::Set(Some(hash)),
        created_at: ActiveValue::Set(ts),
        updated_at: ActiveValue::Set(ts),
        ..Default::default()
    })
    .exec_with_returning(&state.db)
    .await;
    match inserted {
        Ok(created) => (StatusCode::CREATED, Json(user_info(&created))).into_response(),
        Err(_) => api_error(StatusCode::CONFLICT, "username already exists"),
    }
}

/// `DELETE /api/users/{id}` — anyone authenticated, but never yourself.
pub(crate) async fn handle_delete(
    State(state): State<UiState>,
    Extension(MaybeIdentity(ident)): Extension<MaybeIdentity>,
    Path(id): Path<i64>,
) -> Response {
    let ident = match require(&ident) {
        Ok(ident) => ident,
        Err(res) => return *res,
    };
    if ident.user_id == id {
        return api_error(StatusCode::FORBIDDEN, "cannot delete yourself");
    }
    // oidc identities belong to the provider: a delete here would either brick
    // the user (provisioning cache) or silently undo itself on their next visit.
    match user::Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(target)) if target.auth_source == "oidc" => {
            return api_error(
                StatusCode::CONFLICT,
                "oidc users are managed by the identity provider",
            );
        }
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "no such user"),
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database error"),
    }
    match user::Entity::delete_by_id(id).exec(&state.db).await {
        Ok(res) if res.rows_affected > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => api_error(StatusCode::NOT_FOUND, "no such user"),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "database error"),
    }
}
