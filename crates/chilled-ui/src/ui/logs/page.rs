//! The Logs page: filter controls, follow toggle, colorized line view.
//!
//! Lines land straight in a bounded signal; server log rates are far below
//! anything that would need out-of-graph batching.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use chilled_wire::LogLine;
use dioxus::prelude::*;

use super::line::{format_ts, level_class, rank};
use super::stream::{self, LogStream};

/// Rendered tail after filtering, to bound DOM size.
const RENDER_MAX: usize = 2000;

/// The scrollable log container, if mounted.
fn log_scroll_element() -> Option<web_sys::Element> {
    web_sys::window()?
        .document()?
        .get_element_by_id("log-scroll")
}

#[component]
pub fn Logs() -> Element {
    let lines = use_signal(VecDeque::<LogLine>::new);
    let mut connected = use_signal(|| true);
    let gap = use_signal(|| 0u64);
    let mut follow = use_signal(|| true);
    let mut search = use_signal(String::new);
    let mut min_level = use_signal(|| "trace".to_string());

    // The stream lives as long as the page; use_drop closes it and releases
    // the closures when navigating away.
    let source = use_hook(|| Rc::new(RefCell::new(Option::<LogStream>::None)));
    let hooks = source.clone();
    use_effect(move || {
        if hooks.borrow().is_some() {
            return;
        }
        match stream::connect(lines, connected, gap) {
            Some(stream) => *hooks.borrow_mut() = Some(stream),
            None => connected.set(false),
        }
    });
    let cleanup = source.clone();
    use_drop(move || {
        if let Some(stream) = cleanup.borrow_mut().take() {
            stream.close();
        }
    });

    // After each render with new lines, keep the view pinned to the bottom
    // while following.
    use_effect(move || {
        let _ = lines.read().len();
        if !follow() {
            return;
        }
        if let Some(el) = log_scroll_element() {
            el.set_scroll_top(el.scroll_height());
        }
    });

    let visible: Vec<LogLine> = {
        let needle = search().to_lowercase();
        let min = rank(&min_level());
        let buf = lines.read();
        let filtered: Vec<LogLine> = buf
            .iter()
            .filter(|l| rank(&l.level) >= min)
            .filter(|l| {
                needle.is_empty()
                    || l.msg.to_lowercase().contains(&needle)
                    || l.target.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        let skip = filtered.len().saturating_sub(RENDER_MAX);
        filtered.into_iter().skip(skip).collect()
    };
    let shown = visible.len();
    let total = lines.read().len();

    rsx! {
        h1 { "Logs" }
        div { class: "table-controls",
            input {
                class: "input search",
                r#type: "search",
                placeholder: "Filter logs…",
                value: "{search}",
                oninput: move |evt| search.set(evt.value()),
            }
            select {
                class: "select",
                value: "{min_level}",
                onchange: move |evt| min_level.set(evt.value()),
                option { value: "trace", "trace+" }
                option { value: "debug", "debug+" }
                option { value: "info", "info+" }
                option { value: "warn", "warn+" }
                option { value: "error", "error" }
            }
            button {
                class: if follow() { "btn btn-primary btn-sm" } else { "btn btn-sm" },
                onclick: move |_| follow.set(!follow()),
                if follow() { "Following" } else { "Follow" }
            }
            if !connected() {
                span { class: "badge warn-badge", "reconnecting…" }
            }
            if gap() > 0 {
                span { class: "badge warn-badge", "{gap()} lines dropped" }
            }
            span { class: "muted small", "{shown} / {total} lines" }
        }
        div {
            id: "log-scroll",
            class: "log-view",
            onscroll: move |_| {
                if let Some(el) = log_scroll_element() {
                    let at_bottom =
                        el.scroll_height() - el.scroll_top() <= el.client_height() + 4;
                    if follow() != at_bottom {
                        follow.set(at_bottom);
                    }
                }
            },
            if visible.is_empty() {
                div { class: "muted center", "No log lines (yet)." }
            }
            for line in visible.iter() {
                div { class: level_class(&line.level), key: "{line.seq}",
                    span { class: "log-ts", "{format_ts(line.ts_ms)}" }
                    span { class: "log-level", "{line.level}" }
                    span { class: "log-target", "{line.target}" }
                    span { class: "log-msg", "{line.msg}" }
                }
            }
        }
    }
}
