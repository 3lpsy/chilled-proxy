//! Builtin-mode login form (oidc mode links out to the identity provider).

use chilled_wire::LoginReq;
use dioxus::prelude::*;

use crate::api;
use crate::routes::Route;
use crate::session::use_session;

#[component]
pub fn Login() -> Element {
    let session = use_session();
    let nav = use_navigator();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut busy = use_signal(|| false);

    if let Some(url) = session.meta().and_then(|m| m.login_url) {
        return rsx! {
            div { class: "card form-card",
                h1 { "Login" }
                p { "Sign-in is handled by your identity provider." }
                a { class: "btn btn-primary", href: "{url}", "Continue to login" }
            }
        };
    }

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            let req = LoginReq {
                username: username(),
                password: password(),
            };
            match api::send_empty("POST", "/api/session", Some(&req)).await {
                Ok(()) => {
                    // Refresh before navigating so the navbar and any route
                    // guards see the fresh identity immediately.
                    session.refresh_now().await;
                    nav.replace(Route::Home {});
                }
                Err(err) => error.set(Some(err.to_string())),
            }
            busy.set(false);
        });
    };

    rsx! {
        form { class: "card form-card", onsubmit: submit,
            h1 { "Login" }
            if let Some(msg) = error() {
                div { class: "error-banner", "{msg}" }
            }
            label { class: "field",
                span { "Username" }
                input {
                    class: "input",
                    value: "{username}",
                    autocomplete: "username",
                    oninput: move |evt| username.set(evt.value()),
                }
            }
            label { class: "field",
                span { "Password" }
                input {
                    class: "input",
                    r#type: "password",
                    value: "{password}",
                    autocomplete: "current-password",
                    oninput: move |evt| password.set(evt.value()),
                }
            }
            button { class: "btn btn-primary", r#type: "submit", disabled: busy(),
                if busy() { "Signing in…" } else { "Sign in" }
            }
        }
    }
}
