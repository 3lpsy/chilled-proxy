//! The layout every page renders inside: navbar on top, content below.

use dioxus::prelude::*;

use crate::routes::Route;
use crate::session::{use_session, SessionState};
use crate::ui::navbar::Navbar;

#[component]
pub fn Shell() -> Element {
    let session = use_session();
    let nav = use_navigator();
    let route = use_route::<Route>();

    // First-user mode: everything routes to setup until an account exists.
    // `use_reactive` tracks the route, so the guard re-fires on navigation.
    use_effect(use_reactive!(|route| {
        let needs_setup = session.meta().is_some_and(|m| m.needs_first_user);
        if needs_setup && route != (Route::Setup {}) {
            nav.replace(Route::Setup {});
        }
    }));

    let bootstrap_error = match &*session.state.read() {
        SessionState::Error(err) => Some(err.clone()),
        _ => None,
    };

    rsx! {
        Navbar {}
        main { class: "content",
            if let Some(err) = bootstrap_error {
                div { class: "card error-banner banner-row",
                    span { "Could not reach the server: {err}" }
                    button { class: "btn btn-sm", onclick: move |_| session.refresh(), "Retry" }
                }
            }
            Outlet::<Route> {}
        }
    }
}
