//! `POST/DELETE /api/session` — builtin-mode login and logout.

use std::sync::OnceLock;

use axum::extract::State;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chilled_wire::LoginReq;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::api_error;
use crate::authn::{self, password, session};
use crate::config::AuthMode;
use crate::db::entity::{session as session_row, user};
use crate::state::UiState;

/// A real PHC string verified when the user doesn't exist, so a username probe
/// costs the same as a wrong password.
fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| password::hash("dummy-timing-equalizer").expect("argon2 works"))
}

pub(crate) async fn handle_login(
    State(state): State<UiState>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> Response {
    if state.config.auth_mode == AuthMode::Oidc {
        return api_error(
            StatusCode::CONFLICT,
            "login is managed by the identity provider",
        );
    }
    let found = user::Entity::find()
        .filter(user::Column::Username.eq(req.username.trim()))
        .one(&state.db)
        .await;
    let Ok(found) = found else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "database error");
    };
    let (verified, user_id) = match &found {
        Some(u) => match &u.password_hash {
            Some(phc) => (password::verify(&req.password, phc), u.id),
            None => (password::verify(&req.password, dummy_hash()) && false, 0),
        },
        None => (password::verify(&req.password, dummy_hash()) && false, 0),
    };
    if !verified {
        return api_error(StatusCode::UNAUTHORIZED, "invalid username or password");
    }
    match authn::create_session(&state, user_id, authn::forwarded_https(&headers)).await {
        Ok(cookie) => ([(SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response(),
        Err(err) => {
            log::error!("ui: {err}");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "session creation failed")
        }
    }
}

pub(crate) async fn handle_logout(State(state): State<UiState>, headers: HeaderMap) -> Response {
    for header in headers.get_all(COOKIE) {
        let Ok(raw) = header.to_str() else { continue };
        for token in session::cookie_values(raw) {
            let _ = session_row::Entity::delete_many()
                .filter(session_row::Column::TokenHash.eq(session::token_hash(token)))
                .exec(&state.db)
                .await;
        }
    }
    (
        [(SET_COOKIE, session::clear_cookie())],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}
