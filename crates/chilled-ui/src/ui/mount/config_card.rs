//! The card showing one mount's redacted configuration.

use chilled_wire::MountConfig;
use dioxus::prelude::*;

use crate::format::{cooldown_label, human_size};
use crate::ui::widgets::AuthBadges;

#[component]
pub fn MountConfigCard(cfg: MountConfig) -> Element {
    let cooldown = cooldown_label(cfg.cooldown_secs);
    rsx! {
        div { class: "card config-card",
            div { class: "config-grid",
                div { span { class: "muted", "Cached artifacts" }
                strong { "{cfg.artifact_count}" }
            }
            div { span { class: "muted", "Total size" }
                strong { "{human_size(cfg.total_size_bytes)}" }
            }
            div { span { class: "muted", "Mount path" } code { "{cfg.path}" } }
                div { span { class: "muted", "Upstream" } code { "{cfg.upstream}" } }
                if let Some(secondary) = &cfg.secondary {
                    div { span { class: "muted", "Secondary" } code { "{secondary}" } }
                }
                div { span { class: "muted", "Proxy URL" } code { "{cfg.proxy_url}" } }
                div { span { class: "muted", "Cooldown" } span { "{cooldown}" } }
                div { span { class: "muted", "Cache TTL" } span { "{cfg.cache_ttl_secs}s" } }
                div { span { class: "muted", "Restrict downloads" }
                    span { if cfg.restrict_downloads { "yes" } else { "no" } }
                }
                div { span { class: "muted", "Upstream auth" }
                    AuthBadges { auth: cfg.auth.clone() }
                }
            }
        }
    }
}
