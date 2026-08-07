//! First-user creation, reached automatically while no account exists.

use chilled_wire::CreateUserReq;
use dioxus::prelude::*;

use crate::api;
use crate::routes::Route;
use crate::session::use_session;

#[component]
pub fn Setup() -> Element {
    let session = use_session();
    let nav = use_navigator();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut confirm = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut busy = use_signal(|| false);

    if session.meta().is_some_and(|m| !m.needs_first_user) {
        return rsx! {
            div { class: "card notice",
                p { "Setup is complete." }
                Link { class: "btn", to: Route::Home {}, "Go home" }
            }
        };
    }

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() {
            return;
        }
        if password() != confirm() {
            error.set(Some("passwords do not match".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            let req = CreateUserReq {
                username: username(),
                password: password(),
            };
            match api::send_json::<_, chilled_wire::UserInfo>("POST", "/api/setup/first-user", &req)
                .await
            {
                Ok(_) => {
                    // Refresh first: navigating re-fires the first-user route
                    // guard, which must see needs_first_user = false.
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
            h1 { "Welcome" }
            p { class: "muted",
                "No users exist yet. Create the first account — it becomes the login for this UI."
            }
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
                span { "Password (min 8 characters)" }
                input {
                    class: "input",
                    r#type: "password",
                    value: "{password}",
                    autocomplete: "new-password",
                    oninput: move |evt| password.set(evt.value()),
                }
            }
            label { class: "field",
                span { "Confirm password" }
                input {
                    class: "input",
                    r#type: "password",
                    value: "{confirm}",
                    autocomplete: "new-password",
                    oninput: move |evt| confirm.set(evt.value()),
                }
            }
            button { class: "btn btn-primary", r#type: "submit", disabled: busy(),
                if busy() { "Creating…" } else { "Create account" }
            }
        }
    }
}
