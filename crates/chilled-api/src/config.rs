//! Resolved UI configuration, produced by the CLI layer once at startup.

use std::path::PathBuf;
use std::time::Duration;

pub use chilled_wire::AuthMode;

/// Everything the UI runtime needs, validated before the server binds.
#[derive(Debug, Clone)]
pub struct UiConfig {
    pub auth_mode: AuthMode,
    /// Trusted identity header (oidc mode only), lowercase.
    pub oidc_user_header: Option<String>,
    /// Where the navbar Login button points in oidc mode.
    pub oidc_login_url: Option<String>,
    pub public_readonly: bool,
    pub cache_update_interval: Duration,
    pub trust_first_user_signup: bool,
    pub admin_username: Option<String>,
    pub admin_password: Option<String>,
    pub db_path: PathBuf,
    pub session_ttl: Duration,
    /// Dev override: serve the frontend from this directory instead of the embed.
    pub dev_dist_dir: Option<PathBuf>,
}
