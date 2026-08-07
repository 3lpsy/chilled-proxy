//! Top navbar: brand, mount links (overflowing into "More"), login/user menu.

use dioxus::prelude::*;

use crate::api;
use crate::format::friendly_name;
use crate::routes::Route;
use crate::session::use_session;

/// Mount links shown inline on desktop before overflowing into "More ▾".
const INLINE_MOUNTS: usize = 6;

#[component]
pub fn Navbar() -> Element {
    let session = use_session();
    let mut drawer_open = use_signal(|| false);
    let mut more_open = use_signal(|| false);
    let mut user_open = use_signal(|| false);

    let meta = session.meta();
    let mounts = meta.as_ref().map(|m| m.mounts.clone()).unwrap_or_default();
    let (inline, overflow) = if mounts.len() > INLINE_MOUNTS {
        mounts.split_at(INLINE_MOUNTS)
    } else {
        (&mounts[..], &[][..])
    };
    let user = meta.as_ref().and_then(|m| m.user.clone());
    let builtin = meta
        .as_ref()
        .is_some_and(|m| m.auth_mode == chilled_wire::AuthMode::Builtin);
    let login_url = meta.as_ref().and_then(|m| m.login_url.clone());

    let mut close_all = move || {
        drawer_open.set(false);
        more_open.set(false);
        user_open.set(false);
    };

    let any_open = drawer_open() || more_open() || user_open();

    rsx! {
        // Clicking anywhere outside an open menu closes it. The overlay sits
        // above the page but below the navbar and its dropdowns.
        if any_open {
            div { class: "menu-overlay", onclick: move |_| close_all() }
        }
        header { class: "navbar",
            button {
                class: "hamburger",
                aria_label: "Menu",
                onclick: move |_| {
                    let open = drawer_open();
                    close_all();
                    drawer_open.set(!open);
                },
                "☰"
            }
            Link { class: "brand", to: Route::Home {}, onclick: move |_| close_all(),
                "chilled-proxy"
            }
            nav { class: if drawer_open() { "nav-links nav-open" } else { "nav-links" },
                for mount in inline.iter().cloned() {
                    Link {
                        class: "nav-link",
                        to: Route::Mount { name: mount.name.clone() },
                        onclick: move |_| close_all(),
                        "{friendly_name(&mount.name)}"
                    }
                }
                if !overflow.is_empty() {
                    div { class: "dropdown",
                        button {
                            class: "nav-link",
                            onclick: move |_| {
                                let open = more_open();
                                close_all();
                                more_open.set(!open);
                            },
                            "More ▾"
                        }
                        if more_open() {
                            div { class: "dropdown-menu",
                                for mount in overflow.iter().cloned() {
                                    Link {
                                        class: "dropdown-item",
                                        to: Route::Mount { name: mount.name.clone() },
                                        onclick: move |_| close_all(),
                                        "{friendly_name(&mount.name)}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "nav-right",
                match user {
                    Some(user) => {
                        let initial = user.username.chars().next().unwrap_or('?').to_uppercase().to_string();
                        rsx! {
                            div { class: "dropdown",
                                button {
                                    class: "avatar",
                                    title: "{user.username}",
                                    onclick: move |_| {
                                        let open = user_open();
                                        close_all();
                                        user_open.set(!open);
                                    },
                                    "{initial}"
                                }
                                if user_open() {
                                    div { class: "dropdown-menu dropdown-right",
                                        span { class: "dropdown-header", "{user.username}" }
                                        Link { class: "dropdown-item", to: Route::Profile {}, onclick: move |_| close_all(), "Profile" }
                                        Link { class: "dropdown-item", to: Route::ServerConfig {}, onclick: move |_| close_all(), "Configuration" }
                                        Link { class: "dropdown-item", to: Route::Users {}, onclick: move |_| close_all(), "User Management" }
                                        Link { class: "dropdown-item", to: Route::Logs {}, onclick: move |_| close_all(), "Logs" }
                                        if builtin {
                                            button {
                                                class: "dropdown-item",
                                                onclick: move |_| {
                                                    close_all();
                                                    spawn(async move {
                                                        let _ = api::send_empty::<()>("DELETE", "/api/session", None).await;
                                                        session.refresh();
                                                    });
                                                },
                                                "Logout"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None => match login_url {
                        Some(url) => rsx! { a { class: "btn btn-primary btn-sm", href: "{url}", "Login" } },
                        None => rsx! {
                            Link { class: "btn btn-primary btn-sm", to: Route::Login {}, onclick: move |_| close_all(), "Login" }
                        },
                    },
                }
            }
        }
    }
}
