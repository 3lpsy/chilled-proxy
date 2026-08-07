//! Opaque session tokens and the cookie that carries them (cookies suit the
//! logs SSE handshake). The database stores only each token's SHA-256.

use axum::http::HeaderMap;
use sea_orm::{ActiveValue, EntityTrait};
use sha2::{Digest, Sha256};

use crate::db::entity::session as session_row;
use crate::state::UiState;
use crate::time::now;

/// Session cookie name.
pub(crate) const COOKIE: &str = "chilled_session";

/// Mints a fresh opaque token (32 random bytes, hex).
pub(crate) fn mint_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("cannot generate session token: {e}"))?;
    Ok(hex(&bytes))
}

/// The stored form of a token: SHA-256, hex.
pub(crate) fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every `chilled_session` value in a Cookie header — all are checked, not
/// just the first: browsers may send stale duplicates from other paths.
pub(crate) fn cookie_values(header: &str) -> Vec<&str> {
    header
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (name.trim() == COOKIE).then(|| value.trim())
        })
        .filter(|v| !v.is_empty())
        .collect()
}

/// Builds the Set-Cookie value for a fresh session. `Secure` tracks the
/// forwarded proto: unconditional `Secure` would break plain-HTTP LAN use.
pub(crate) fn set_cookie(token: &str, max_age_secs: u64, https: bool) -> String {
    format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age_secs}{}",
        if https { "; Secure" } else { "" }
    )
}

/// Builds the Set-Cookie value that clears the session cookie.
pub(crate) fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// Whether the client reached us over HTTPS, per the reverse proxy.
pub(crate) fn forwarded_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').next().unwrap_or("").trim() == "https")
        .unwrap_or(false)
}

/// Mints a session row for a user and returns the Set-Cookie value.
pub(crate) async fn create_session(
    state: &UiState,
    user_id: i64,
    https: bool,
) -> Result<String, String> {
    let token = mint_token()?;
    let ttl = state.config.session_ttl.as_secs();
    let row = session_row::ActiveModel {
        token_hash: ActiveValue::Set(token_hash(&token)),
        user_id: ActiveValue::Set(user_id),
        created_at: ActiveValue::Set(now()),
        expires_at: ActiveValue::Set(now() + ttl as i64),
        ..Default::default()
    };
    session_row::Entity::insert(row)
        .exec(&state.db)
        .await
        .map_err(|e| format!("session creation failed: {e}"))?;
    Ok(set_cookie(&token, ttl, https))
}
