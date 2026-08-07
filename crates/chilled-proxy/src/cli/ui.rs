//! Resolving and validating the `--ui-*` flags into a [`UiConfig`].

use std::path::PathBuf;
use std::time::Duration;

use axum::http::HeaderName;
use chilled_api::{AuthMode, UiConfig};

use crate::cli::Cli;
use crate::constants::{DEFAULT_UI_DB_PATH, MIN_UI_CACHE_UPDATE_INTERVAL_SECS};

/// Default snapshot interval when `--ui-cache-update-interval` is not given.
pub(super) const DEFAULT_CACHE_UPDATE_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Default session lifetime when `--ui-session-ttl` is not given.
pub(super) const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// Resolves the UI configuration, or `None` when `--ui` is off. A `--ui-*`
/// flag without `--ui` is an error: half-configured intent is worth reporting.
pub(crate) fn resolve_ui(cli: &Cli) -> Result<Option<UiConfig>, String> {
    if !cli.ui {
        if let Some(flag) = set_ui_flag(cli) {
            return Err(format!("{flag} is set but --ui is not enabled"));
        }
        return Ok(None);
    }

    let auth_mode = match cli.ui_auth.as_deref() {
        None | Some("builtin") => AuthMode::Builtin,
        Some("oidc") => AuthMode::Oidc,
        Some(other) => return Err(format!("invalid --ui-auth '{other}': use builtin or oidc")),
    };

    let oidc_user_header = match (auth_mode, &cli.ui_oidc_user_header) {
        (AuthMode::Oidc, Some(name)) => Some(valid_header_name(name)?),
        (AuthMode::Oidc, None) => {
            return Err("--ui-auth oidc requires --ui-oidc-user-header".into());
        }
        (AuthMode::Builtin, Some(_)) => {
            return Err("--ui-oidc-user-header requires --ui-auth oidc".into());
        }
        (AuthMode::Builtin, None) => None,
    };
    if auth_mode == AuthMode::Builtin && cli.ui_oidc_login_url.is_some() {
        return Err("--ui-oidc-login-url requires --ui-auth oidc".into());
    }
    // The value lands in an <a href>; a javascript: URL would execute.
    if let Some(login_url) = &cli.ui_oidc_login_url {
        let ok = login_url.starts_with('/')
            || login_url.starts_with("https://")
            || login_url.starts_with("http://");
        if !ok {
            return Err(format!(
                "--ui-oidc-login-url '{login_url}' must be a path or an http(s) URL"
            ));
        }
    }

    if auth_mode == AuthMode::Oidc && cli.ui_trust_first_user_signup {
        return Err("--ui-trust-first-user-signup is incompatible with --ui-auth oidc".into());
    }
    match (auth_mode, &cli.ui_admin_username, &cli.ui_admin_password) {
        (AuthMode::Oidc, Some(_), _) | (AuthMode::Oidc, _, Some(_)) => {
            return Err("--ui-admin-username/--ui-admin-password are incompatible with --ui-auth oidc (users come from the identity provider)".into());
        }
        (_, Some(_), None) | (_, None, Some(_)) => {
            return Err("--ui-admin-username and --ui-admin-password must be set together".into());
        }
        _ => {}
    }

    let interval = cli
        .ui_cache_update_interval
        .unwrap_or(DEFAULT_CACHE_UPDATE_INTERVAL);
    if interval.as_secs() < MIN_UI_CACHE_UPDATE_INTERVAL_SECS {
        return Err(format!(
            "--ui-cache-update-interval must be at least {MIN_UI_CACHE_UPDATE_INTERVAL_SECS}s"
        ));
    }
    let session_ttl = cli.ui_session_ttl.unwrap_or(DEFAULT_SESSION_TTL);
    if session_ttl.is_zero() {
        return Err("--ui-session-ttl must be greater than zero".into());
    }
    let db_path = cli
        .ui_db_path
        .clone()
        .unwrap_or_else(|| DEFAULT_UI_DB_PATH.to_string());
    if db_path.is_empty() {
        return Err("--ui-db-path must not be empty".into());
    }

    Ok(Some(UiConfig {
        auth_mode,
        oidc_user_header,
        oidc_login_url: cli.ui_oidc_login_url.clone(),
        public_readonly: cli.ui_public_readonly_enabled,
        cache_update_interval: interval,
        trust_first_user_signup: cli.ui_trust_first_user_signup,
        admin_username: cli.ui_admin_username.clone(),
        admin_password: cli.ui_admin_password.clone(),
        db_path: PathBuf::from(db_path),
        session_ttl,
        dev_dist_dir: cli.ui_dev_dist_dir.clone().map(PathBuf::from),
    }))
}

/// The first `--ui-*` flag that was set, for the without-`--ui` error.
fn set_ui_flag(cli: &Cli) -> Option<&'static str> {
    [
        (cli.ui_auth.is_some(), "--ui-auth"),
        (cli.ui_oidc_user_header.is_some(), "--ui-oidc-user-header"),
        (cli.ui_oidc_login_url.is_some(), "--ui-oidc-login-url"),
        (
            cli.ui_public_readonly_enabled,
            "--ui-public-readonly-enabled",
        ),
        (
            cli.ui_cache_update_interval.is_some(),
            "--ui-cache-update-interval",
        ),
        (
            cli.ui_trust_first_user_signup,
            "--ui-trust-first-user-signup",
        ),
        (cli.ui_admin_username.is_some(), "--ui-admin-username"),
        (cli.ui_admin_password.is_some(), "--ui-admin-password"),
        (cli.ui_db_path.is_some(), "--ui-db-path"),
        (cli.ui_session_ttl.is_some(), "--ui-session-ttl"),
        (cli.ui_dev_dist_dir.is_some(), "--ui-dev-dist-dir"),
    ]
    .into_iter()
    .find_map(|(set, name)| set.then_some(name))
}

/// Validates and lowercases a trusted-header name.
fn valid_header_name(name: &str) -> Result<String, String> {
    let lower = name.to_ascii_lowercase();
    HeaderName::try_from(lower.as_str())
        .map(|_| lower)
        .map_err(|_| format!("invalid --ui-oidc-user-header '{name}'"))
}
