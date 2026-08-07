//! Catch-all for unknown client routes.

use dioxus::prelude::*;

use crate::routes::Route;

#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        div { class: "card notice",
            h1 { "Not found" }
            p { class: "muted", "/{segments.join(\"/\")}" }
            Link { class: "btn", to: Route::Home {}, "Go home" }
        }
    }
}
