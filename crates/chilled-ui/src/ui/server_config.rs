//! View-only server configuration.

use chilled_wire::ServerConfig as ServerConfigDto;
use dioxus::prelude::*;

use crate::api;
use crate::format::{cooldown_label, friendly_name};
use crate::ui::widgets::{AuthBadges, ErrorState, Loading};

#[component]
pub fn ServerConfig() -> Element {
    let config = use_resource(|| async { api::get_json::<ServerConfigDto>("/api/config").await });
    rsx! {
        h1 { "Configuration" }
        p { class: "muted", "View-only. Credential values are redacted; header names are shown." }
        match &*config.read() {
            None => rsx! { Loading {} },
            Some(Err(err)) => rsx! { ErrorState { err: err.clone(), what: "the configuration".to_string() } },
            Some(Ok(cfg)) => rsx! {
                div { class: "card config-card",
                    h2 { "Server" }
                    div { class: "config-grid",
                        div { span { class: "muted", "Version" } code { "{cfg.version}" } }
                        div { span { class: "muted", "Listen" } code { "{cfg.listen}" } }
                        div { span { class: "muted", "Log level" } code { "{cfg.log_level}" } }
                        div { span { class: "muted", "Metrics" } span { if cfg.metrics_enabled { "enabled" } else { "disabled" } } }
                        if !cfg.disabled.is_empty() {
                            div { span { class: "muted", "Disabled registries" } span { "{cfg.disabled.join(\", \")}" } }
                        }
                    }
                }
                div { class: "card config-card",
                    h2 { "Web UI" }
                    div { class: "config-grid",
                        div { span { class: "muted", "Auth mode" }
                            code { if cfg.ui.auth_mode == chilled_wire::AuthMode::Oidc { "oidc" } else { "builtin" } } }
                        div { span { class: "muted", "Public readonly" } span { if cfg.ui.public_readonly { "yes" } else { "no" } } }
                        div { span { class: "muted", "Snapshot interval" } span { "{cfg.ui.cache_update_interval_secs}s" } }
                        div { span { class: "muted", "Database" } code { "{cfg.ui.db_path}" } }
                        div { span { class: "muted", "Trust first-user signup" } span { if cfg.ui.trust_first_user_signup { "yes" } else { "no" } } }
                        div { span { class: "muted", "Session TTL" } span { "{cfg.ui.session_ttl_secs}s" } }
                    }
                }
                for mount in cfg.mounts.iter() {
                    div { class: "card config-card",
                        h2 { "{friendly_name(&mount.name)}" span { class: "badge", "{mount.kind}" } }
                        div { class: "config-grid",
                            div { span { class: "muted", "Path" } code { "{mount.path}" } }
                            div { span { class: "muted", "Upstream" } code { "{mount.upstream}" } }
                            if let Some(secondary) = &mount.secondary {
                                div { span { class: "muted", "Secondary" } code { "{secondary}" } }
                            }
                            div { span { class: "muted", "Proxy URL" } code { "{mount.proxy_url}" } }
                            div { span { class: "muted", "Cooldown" }
                                span { "{cooldown_label(mount.cooldown_secs)}" } }
                            div { span { class: "muted", "Cache TTL" } span { "{mount.cache_ttl_secs}s" } }
                            div { span { class: "muted", "Restrict downloads" } span { if mount.restrict_downloads { "yes" } else { "no" } } }
                            div { span { class: "muted", "Auth" }
                                AuthBadges { auth: mount.auth.clone() }
                            }
                        }
                    }
                }
            },
        }
    }
}
