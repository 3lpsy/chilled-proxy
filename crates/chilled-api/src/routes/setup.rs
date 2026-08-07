//! `POST /api/setup/first-user` — trusted first-visitor account creation.

use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chilled_wire::CreateUserReq;
use sea_orm::{ActiveValue, EntityTrait, PaginatorTrait, TransactionTrait};

use super::api_error;
use crate::authn::{self, password};
use crate::config::AuthMode;
use crate::db::entity::user;
use crate::state::UiState;
use crate::time::now;

/// Minimum password length for created accounts.
pub(crate) const MIN_PASSWORD_LEN: usize = 8;

/// Validates a username/password pair for account creation.
pub(crate) fn check_new_credentials(username: &str, pw: &str) -> Result<(), &'static str> {
    if username.is_empty() || username.len() > 64 {
        return Err("username must be 1-64 characters");
    }
    if pw.len() < MIN_PASSWORD_LEN {
        return Err("password must be at least 8 characters");
    }
    Ok(())
}

pub(crate) async fn handle_first_user(
    State(state): State<UiState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Response {
    if state.config.auth_mode != AuthMode::Builtin || !state.config.trust_first_user_signup {
        return api_error(StatusCode::CONFLICT, "first-user signup is not enabled");
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

    // The zero-count check runs inside the transaction; the unique username
    // index makes a lost race a clean error rather than a second account.
    let created = state
        .db
        .transaction::<_, user::Model, sea_orm::DbErr>(|txn| {
            Box::pin(async move {
                if user::Entity::find().count(txn).await? > 0 {
                    return Err(sea_orm::DbErr::Custom("users already exist".into()));
                }
                let ts = now();
                let row = user::ActiveModel {
                    username: ActiveValue::Set(username),
                    auth_source: ActiveValue::Set("builtin".to_owned()),
                    password_hash: ActiveValue::Set(Some(hash)),
                    created_at: ActiveValue::Set(ts),
                    updated_at: ActiveValue::Set(ts),
                    ..Default::default()
                };
                let res = user::Entity::insert(row).exec_with_returning(txn).await?;
                Ok(res)
            })
        })
        .await;

    let created = match created {
        Ok(created) => created,
        Err(_) => return api_error(StatusCode::CONFLICT, "a user already exists"),
    };
    match authn::create_session(&state, created.id, authn::forwarded_https(&headers)).await {
        Ok(cookie) => (
            StatusCode::CREATED,
            [(SET_COOKIE, cookie)],
            Json(super::meta::user_info(&created)),
        )
            .into_response(),
        Err(err) => {
            log::error!("ui: {err}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "session creation failed")
        }
    }
}
