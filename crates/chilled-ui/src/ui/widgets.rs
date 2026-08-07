//! Small shared pieces: cards, error banners, login prompts.

use chilled_wire::AuthSummary;
use dioxus::prelude::*;

use crate::api::ApiError;

/// Upstream auth presence as badges: basic auth plus custom header names
/// (values are redacted server-side).
#[component]
pub fn AuthBadges(auth: AuthSummary) -> Element {
    rsx! {
        span { class: "badges",
            if auth.basic { span { class: "badge", "basic auth" } }
            for header in auth.header_names.iter() {
                span { class: "badge", "{header}" }
            }
            if !auth.basic && auth.header_names.is_empty() {
                span { class: "muted", "none" }
            }
        }
    }
}

/// A note that data needs auth. No button of its own: Login always lives in
/// the top-right of the navbar, and a second one here was just noise.
#[component]
pub fn LoginPrompt(what: String) -> Element {
    rsx! {
        div { class: "card notice",
            p { "Sign in (top right) to view {what}." }
        }
    }
}

/// Renders an API error: a login prompt for 401, a plain banner otherwise.
#[component]
pub fn ErrorState(err: ApiError, what: String) -> Element {
    match err {
        ApiError::Unauthorized => rsx! { LoginPrompt { what } },
        other => rsx! { div { class: "card error-banner", "{other}" } },
    }
}

/// Loading placeholder.
#[component]
pub fn Loading() -> Element {
    rsx! { div { class: "card muted", "Loading…" } }
}
