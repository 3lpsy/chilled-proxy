//! Wire types shared by the management API (server) and the web UI (wasm).
//! Serde-only: this crate must compile for both the host and wasm32 targets.

use serde::{Deserialize, Serialize};

/// How the UI authenticates users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// Username/password accounts managed by the server itself.
    Builtin,
    /// Identity from a trusted reverse-proxy header (oauth2-proxy style).
    Oidc,
}

/// A user as reported by the API. `auth_source` matches [`AuthMode`] values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub auth_source: AuthMode,
    pub created_at: i64,
}

/// One mount as listed in the navbar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountSummary {
    pub name: String,
    pub kind: String,
    pub path: String,
}

/// `GET /api/meta` — the bootstrap document driving navbar and routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub version: String,
    pub auth_mode: AuthMode,
    pub public_readonly: bool,
    pub needs_first_user: bool,
    pub user: Option<UserInfo>,
    pub login_url: Option<String>,
    pub mounts: Vec<MountSummary>,
}

/// Upstream auth presence: names only, never values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSummary {
    pub basic: bool,
    pub header_names: Vec<String>,
}

/// One mount's redacted configuration plus cache totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountConfig {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub upstream: String,
    pub secondary: Option<String>,
    pub proxy_url: String,
    pub cooldown_secs: u64,
    pub cache_ttl_secs: u64,
    pub restrict_downloads: bool,
    pub auth: AuthSummary,
    pub artifact_count: i64,
    pub total_size_bytes: i64,
    pub last_snapshot_at: Option<i64>,
}

/// UI-specific knobs echoed in the config view (no secrets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfigView {
    pub auth_mode: AuthMode,
    pub public_readonly: bool,
    pub cache_update_interval_secs: u64,
    pub db_path: String,
    pub trust_first_user_signup: bool,
    pub session_ttl_secs: u64,
}

/// `GET /api/config` — the whole-server view-only configuration report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub version: String,
    pub listen: String,
    pub log_level: String,
    pub metrics_enabled: bool,
    pub disabled: Vec<String>,
    pub ui: UiConfigView,
    pub mounts: Vec<MountConfig>,
}

/// One row of the cached-artifacts table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRow {
    pub id: i64,
    pub mount: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub size_bytes: i64,
    pub cached_at: i64,
    pub upstream: String,
}

/// A completed (or running) snapshot pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub artifact_count: i64,
}

/// `GET /api/artifacts` — one page of the artifacts table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactPage {
    pub items: Vec<ArtifactRow>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub snapshot: Option<SnapshotInfo>,
}

/// One server log line streamed over `GET /api/logs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub seq: u64,
    pub ts_ms: i64,
    pub level: String,
    pub target: String,
    pub msg: String,
}

/// `POST /api/session` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

/// `POST /api/users` and `POST /api/setup/first-user` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateUserReq {
    pub username: String,
    pub password: String,
}

/// `PATCH /api/users/me` body; unset fields stay unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProfileReq {
    pub current_password: String,
    pub username: Option<String>,
    pub new_password: Option<String>,
}

/// Error envelope every non-2xx API response carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_mode_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&AuthMode::Builtin).unwrap(),
            "\"builtin\""
        );
        assert_eq!(serde_json::to_string(&AuthMode::Oidc).unwrap(), "\"oidc\"");
    }

    #[test]
    fn meta_round_trips() {
        let meta = Meta {
            version: "0.1.6".into(),
            auth_mode: AuthMode::Builtin,
            public_readonly: true,
            needs_first_user: false,
            user: None,
            login_url: None,
            mounts: vec![MountSummary {
                name: "npm".into(),
                kind: "npm".into(),
                path: "/npm".into(),
            }],
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert_eq!(serde_json::from_str::<Meta>(&json).unwrap(), meta);
    }
}
