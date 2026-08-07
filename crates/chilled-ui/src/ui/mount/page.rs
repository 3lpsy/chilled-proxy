//! The page component: mount heading, refresh, config card, artifacts table.

use chilled_wire::MountConfig;
use dioxus::prelude::*;

use crate::api;
use crate::format::friendly_name;
use crate::session::use_session;
use crate::ui::widgets::{ErrorState, Loading};

use super::config_card::MountConfigCard;
use super::table::ArtifactsTable;

#[component]
pub fn Mount(name: String) -> Element {
    let session = use_session();
    let mount_name = use_memo(use_reactive!(|name| name));
    // Bumped after a triggered refresh so config and table refetch.
    let mut reload = use_signal(|| 0u32);
    let mut refreshing = use_signal(|| false);

    let config = use_resource(move || async move {
        let _ = reload();
        api::get_json::<MountConfig>(&format!("/api/registries/{}", mount_name())).await
    });

    // A 401 on the config means the table would 401 too — one prompt, not two.
    let unauthorized = matches!(
        &*config.read(),
        Some(Err(crate::api::ApiError::Unauthorized))
    );

    let logged_in = session.meta().is_some_and(|m| m.user.is_some());
    let clear = move |_| {
        if refreshing() || !logged_in {
            return;
        }
        let confirmed = web_sys::window()
            .and_then(|w| {
                w.confirm_with_message(&format!(
                    "Delete every cached package for {}?",
                    mount_name()
                ))
                .ok()
            })
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        refreshing.set(true);
        spawn(async move {
            let path = format!("/api/registries/{}/clear", mount_name());
            if api::send_empty::<()>("POST", &path, None).await.is_ok() {
                api::sleep_ms(1500).await;
                reload += 1;
            }
            refreshing.set(false);
        });
    };
    let refresh = move |_| {
        if refreshing() || !logged_in {
            return;
        }
        refreshing.set(true);
        spawn(async move {
            let path = format!("/api/registries/{}/refresh", mount_name());
            if api::send_empty::<()>("POST", &path, None).await.is_ok() {
                // The rescan is queued; give it a moment before refetching.
                api::sleep_ms(1500).await;
                reload += 1;
            }
            refreshing.set(false);
        });
    };

    rsx! {
        div { class: "page-head",
            h1 { "{friendly_name(&mount_name())}" }
            div { class: "head-actions",
                button {
                    class: "btn btn-sm",
                    // Present but disabled for public (read-only) viewers.
                    disabled: !logged_in || refreshing(),
                    title: if logged_in { "Rescan this proxy's cache now" } else { "Sign in to refresh" },
                    onclick: refresh,
                    if refreshing() { "Refreshing…" } else { "⟳ Refresh" }
                }
                button {
                    class: "btn btn-sm btn-danger",
                    disabled: !logged_in || refreshing(),
                    title: if logged_in { "Delete every cached package for this proxy" } else { "Sign in to clear" },
                    onclick: clear,
                    "🗑 Clear Cache"
                }
            }
        }
        match &*config.read() {
            None => rsx! { Loading {} },
            Some(Err(err)) => rsx! { ErrorState { err: err.clone(), what: "this proxy".to_string() } },
            Some(Ok(cfg)) => rsx! { MountConfigCard { cfg: cfg.clone() } },
        }
        if !unauthorized {
            ArtifactsTable { mount: mount_name(), reload: reload() }
        }
    }
}
