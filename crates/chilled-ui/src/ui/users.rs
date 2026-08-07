//! User management: list, add, delete (never yourself).

use chilled_wire::{AuthMode, CreateUserReq, UserInfo};
use dioxus::prelude::*;

use crate::api;
use crate::format::{human_time, now_secs};
use crate::session::use_session;
use crate::ui::widgets::{ErrorState, Loading};

#[component]
pub fn Users() -> Element {
    let session = use_session();
    let mut reload = use_signal(|| 0u32);
    let users = use_resource(move || {
        reload();
        async { api::get_json::<Vec<UserInfo>>("/api/users").await }
    });
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);

    let my_id = session
        .meta()
        .and_then(|m| m.user.map(|u| u.id))
        .unwrap_or(-1);
    let builtin = session
        .meta()
        .is_some_and(|m| m.auth_mode == AuthMode::Builtin);

    let create = move |evt: FormEvent| {
        evt.prevent_default();
        error.set(None);
        spawn(async move {
            let req = CreateUserReq {
                username: username(),
                password: password(),
            };
            match api::send_json::<_, UserInfo>("POST", "/api/users", &req).await {
                Ok(_) => {
                    username.set(String::new());
                    password.set(String::new());
                    reload += 1;
                }
                Err(err) => error.set(Some(err.to_string())),
            }
        });
    };

    rsx! {
        h1 { "User Management" }
        if builtin {
            form { class: "card form-row", onsubmit: create,
                input {
                    class: "input",
                    placeholder: "username",
                    value: "{username}",
                    oninput: move |evt| username.set(evt.value()),
                }
                input {
                    class: "input",
                    r#type: "password",
                    placeholder: "password (min 8)",
                    value: "{password}",
                    oninput: move |evt| password.set(evt.value()),
                }
                button { class: "btn btn-primary", r#type: "submit", "＋ Add user" }
            }
        } else {
            div { class: "card notice-banner",
                "Users are provisioned automatically from your identity provider; they can be removed here."
            }
        }
        if let Some(msg) = error() {
            div { class: "error-banner", "{msg}" }
        }
        match &*users.read() {
            None => rsx! { Loading {} },
            Some(Err(err)) => rsx! { ErrorState { err: err.clone(), what: "users".to_string() } },
            Some(Ok(list)) => rsx! {
                div { class: "table-wrap",
                    table { class: "table",
                        thead { tr { th { "Username" } th { "Type" } th { "Created" } th { "" } } }
                        tbody {
                            for user in list.iter().cloned() {
                                tr {
                                    td { "data-label": "Username",
                                        "{user.username}"
                                        if user.id == my_id { span { class: "badge you", "you" } }
                                    }
                                    td { "data-label": "Type",
                                        span { class: "badge", if user.auth_source == AuthMode::Oidc { "oidc" } else { "builtin" } }
                                    }
                                    td { "data-label": "Created", "{human_time(user.created_at, now_secs())}" }
                                    td { class: "row-actions",
                                        button {
                                            class: "btn btn-danger btn-sm",
                                            // Self-delete is refused server-side too.
                                            disabled: user.id == my_id,
                                            onclick: move |_| {
                                                let confirmed = web_sys::window()
                                                    .and_then(|w| w.confirm_with_message(
                                                        &format!("Delete user {}?", user.username)).ok())
                                                    .unwrap_or(false);
                                                if !confirmed { return; }
                                                let id = user.id;
                                                spawn(async move {
                                                    match api::send_empty::<()>("DELETE", &format!("/api/users/{id}"), None).await {
                                                        Ok(()) => reload += 1,
                                                        Err(err) => error.set(Some(err.to_string())),
                                                    }
                                                });
                                            },
                                            "Delete"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
        }
    }
}
