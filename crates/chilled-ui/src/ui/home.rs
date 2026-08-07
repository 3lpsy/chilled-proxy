//! Landing page: one card per mount with cache totals.

use dioxus::prelude::*;

use crate::api;
use crate::format::{friendly_name, human_size, human_time, now_secs};
use crate::routes::Route;
use crate::ui::widgets::{ErrorState, Loading};

#[component]
pub fn Home() -> Element {
    let registries = use_resource(|| async {
        api::get_json::<Vec<chilled_wire::MountConfig>>("/api/registries").await
    });

    rsx! {
        h1 { "Cache overview" }
        match &*registries.read() {
            None => rsx! { Loading {} },
            Some(Err(err)) => rsx! { ErrorState { err: err.clone(), what: "cache state".to_string() } },
            Some(Ok(mounts)) => rsx! {
                div { class: "card-grid",
                    for mount in mounts.iter().cloned() {
                        Link {
                            class: "card mount-card",
                            to: Route::Mount { name: mount.name.clone() },
                            div { class: "mount-card-head",
                                h2 { "{friendly_name(&mount.name)}" }
                                span { class: "badge", "{mount.kind}" }
                            }
                            p { class: "muted", "{mount.path}" }
                            div { class: "mount-card-stats",
                                span { strong { "{mount.artifact_count}" } " artifacts" }
                                span { "{human_size(mount.total_size_bytes)}" }
                            }
                            if let Some(at) = mount.last_snapshot_at {
                                p { class: "muted small", "updated {human_time(at, now_secs())}" }
                            }
                        }
                    }
                }
            },
        }
    }
}
