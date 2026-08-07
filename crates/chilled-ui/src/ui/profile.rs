//! Profile page: edit username/password. oidc accounts see everything
//! grayed out — their identity belongs to the provider.

use chilled_wire::{AuthMode, UpdateProfileReq, UserInfo};
use dioxus::prelude::*;

use crate::api;
use crate::format::{human_time, now_secs};
use crate::session::use_session;
use crate::ui::widgets::{ErrorState, Loading};

#[component]
pub fn Profile() -> Element {
    let me = use_resource(|| async { api::get_json::<UserInfo>("/api/users/me").await });
    rsx! {
        h1 { "Profile" }
        match &*me.read() {
            None => rsx! { Loading {} },
            Some(Err(err)) => rsx! { ErrorState { err: err.clone(), what: "your profile".to_string() } },
            Some(Ok(user)) => rsx! { ProfileForm { user: user.clone() } },
        }
    }
}

#[component]
fn ProfileForm(user: UserInfo) -> Element {
    let session = use_session();
    let oidc = user.auth_source == AuthMode::Oidc;
    let mut username = use_signal(|| user.username.clone());
    let mut current = use_signal(String::new);
    let mut new_password = use_signal(String::new);
    let mut confirm = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut saved = use_signal(|| false);
    let mut busy = use_signal(|| false);

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() || oidc {
            return;
        }
        if new_password() != confirm() {
            error.set(Some("passwords do not match".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        saved.set(false);
        let original = user.username.clone();
        spawn(async move {
            let req = UpdateProfileReq {
                current_password: current(),
                username: (username() != original).then(&*username),
                new_password: (!new_password().is_empty()).then(&*new_password),
            };
            match api::send_json::<_, UserInfo>("PATCH", "/api/users/me", &req).await {
                Ok(_) => {
                    saved.set(true);
                    current.set(String::new());
                    new_password.set(String::new());
                    confirm.set(String::new());
                    session.refresh();
                }
                Err(err) => error.set(Some(err.to_string())),
            }
            busy.set(false);
        });
    };

    rsx! {
        form { class: "card form-card", onsubmit: submit,
            if oidc {
                div { class: "notice-banner",
                    "This account comes from your identity provider; profile fields are read-only here."
                }
            }
            if let Some(msg) = error() {
                div { class: "error-banner", "{msg}" }
            }
            if saved() {
                div { class: "ok-banner", "Profile updated." }
            }
            label { class: "field",
                span { "Username" }
                input {
                    class: "input",
                    value: "{username}",
                    disabled: oidc,
                    oninput: move |evt| username.set(evt.value()),
                }
            }
            label { class: "field",
                span { "Account type" }
                input { class: "input", value: if oidc { "oidc" } else { "builtin" }, disabled: true }
            }
            label { class: "field",
                span { "Member since" }
                input { class: "input", value: "{human_time(user.created_at, now_secs())}", disabled: true }
            }
            fieldset { class: "field-group", disabled: oidc,
                legend { "Change password" }
                label { class: "field",
                    span { "Current password" }
                    input {
                        class: "input",
                        r#type: "password",
                        value: "{current}",
                        autocomplete: "current-password",
                        oninput: move |evt| current.set(evt.value()),
                    }
                }
                label { class: "field",
                    span { "New password (leave empty to keep)" }
                    input {
                        class: "input",
                        r#type: "password",
                        value: "{new_password}",
                        autocomplete: "new-password",
                        oninput: move |evt| new_password.set(evt.value()),
                    }
                }
                label { class: "field",
                    span { "Confirm new password" }
                    input {
                        class: "input",
                        r#type: "password",
                        value: "{confirm}",
                        autocomplete: "new-password",
                        oninput: move |evt| confirm.set(evt.value()),
                    }
                }
            }
            if !oidc {
                button { class: "btn btn-primary", r#type: "submit", disabled: busy(),
                    if busy() { "Saving…" } else { "Save changes" }
                }
            }
        }
    }
}
