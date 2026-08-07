//! The paginated, searchable artifacts table for one mount.

use chilled_wire::ArtifactPage;
use dioxus::prelude::*;

use crate::api;
use crate::format::{absolute_time, human_size, human_time, now_secs};
use crate::ui::widgets::{ErrorState, Loading};

#[component]
pub fn ArtifactsTable(mount: String, reload: u32) -> Element {
    let session = crate::session::use_session();
    let logged_in = session.meta().is_some_and(|m| m.user.is_some());
    let mount = use_memo(use_reactive!(|mount| mount));
    let reload = use_memo(use_reactive!(|reload| reload));
    // Bumped by row actions (delete / repull) to refetch this table alone.
    let mut local_reload = use_signal(|| 0u32);
    let mut page = use_signal(|| 1u64);
    let mut per_page = use_signal(|| 50u64);
    let mut q_input = use_signal(String::new);
    let mut q = use_signal(String::new);
    let mut sort = use_signal(|| "name".to_string());
    let mut order = use_signal(|| "asc".to_string());
    // Bumped on every keystroke; only the newest debounce commits its search.
    let mut generation = use_signal(|| 0u64);

    // Switching mounts is a fresh context, so page and search reset; sort
    // direction and page size are kept as user preferences.
    use_effect(use_reactive!(|mount| {
        let _ = mount;
        page.set(1);
        q.set(String::new());
        q_input.set(String::new());
    }));

    let query = use_memo(move || {
        format!(
            "/api/artifacts?mount={}&page={}&per_page={}&q={}&sort={}&order={}",
            urlencode(&mount()),
            page(),
            per_page(),
            urlencode(&q()),
            sort(),
            order()
        )
    });
    let result = use_resource(move || async move {
        // A bumped reload refetches the same query (post-refresh/actions).
        let _ = reload();
        let _ = local_reload();
        api::get_json::<ArtifactPage>(&query()).await
    });

    let mut debounce = move |value: String| {
        q_input.set(value.clone());
        generation += 1;
        let expect = generation();
        spawn(async move {
            crate::api::sleep_ms(300).await;
            if generation() == expect {
                q.set(value);
                page.set(1);
            }
        });
    };

    rsx! {
        div { class: "table-controls",
            input {
                class: "input search",
                r#type: "search",
                placeholder: "Search artifacts…",
                value: "{q_input}",
                oninput: move |evt| debounce(evt.value()),
            }
            select {
                class: "select",
                value: "{sort}",
                onchange: move |evt| { sort.set(evt.value()); page.set(1); },
                option { value: "name", "Name" }
                option { value: "version", "Version" }
                option { value: "size", "Size" }
                option { value: "cached_at", "Cached" }
            }
            button {
                class: "btn",
                title: "Toggle sort direction",
                onclick: move |_| {
                    order.set(if order() == "asc" { "desc".into() } else { "asc".into() });
                    page.set(1);
                },
                if order() == "asc" { "↑" } else { "↓" }
            }
            select {
                class: "select",
                value: "{per_page}",
                onchange: move |evt| {
                    per_page.set(evt.value().parse().unwrap_or(50));
                    page.set(1);
                },
                option { value: "25", "25" }
                option { value: "50", "50" }
                option { value: "100", "100" }
            }
        }
        match &*result.read() {
            None => rsx! { Loading {} },
            Some(Err(err)) => rsx! { ErrorState { err: err.clone(), what: "cached artifacts".to_string() } },
            Some(Ok(data)) => {
                let total_pages = data.total.div_ceil(data.per_page).max(1);
                let now = now_secs();
                rsx! {
                    div { class: "table-wrap",
                        table { class: "table artifacts",
                            thead {
                                tr {
                                    th { "Name" }
                                    th { "Version" }
                                    th { "Size" }
                                    th { "Cached" }
                                    if logged_in { th { "Actions" } }
                                }
                            }
                            tbody {
                                if data.items.is_empty() {
                                    tr { td { colspan: if logged_in { 5 } else { 4 }, class: "muted center",
                                        if q().is_empty() { "Nothing cached yet — the table fills after the first snapshot." }
                                        else { "No artifacts match." }
                                    } }
                                }
                                for item in data.items.iter() {
                                    tr {
                                        td { "data-label": "Name", "{item.name}" }
                                        td { "data-label": "Version", code { "{item.version}" } }
                                        td { "data-label": "Size", "{human_size(item.size_bytes)}" }
                                        td { "data-label": "Cached", title: "{absolute_time(item.cached_at)}",
                                            "{human_time(item.cached_at, now)}"
                                        }
                                        if logged_in {
                                            td { "data-label": "Actions", class: "row-actions",
                                                RowActions {
                                                    id: item.id,
                                                    label: format!("{} {}", item.name, item.version),
                                                    on_done: move |_| local_reload += 1,
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "pager",
                        button { class: "btn btn-sm", disabled: data.page <= 1,
                            onclick: move |_| page.set(1), "«" }
                        button { class: "btn btn-sm", disabled: data.page <= 1,
                            onclick: move |_| page.set(page().saturating_sub(1).max(1)), "‹" }
                        span { class: "muted", "page {data.page} / {total_pages} · {data.total} artifacts" }
                        button { class: "btn btn-sm", disabled: data.page >= total_pages,
                            onclick: move |_| page.set(page() + 1), "›" }
                        button { class: "btn btn-sm", disabled: data.page >= total_pages,
                            onclick: move |_| page.set(total_pages), "»" }
                    }
                    if let Some(snapshot) = &data.snapshot {
                        if let Some(at) = snapshot.finished_at {
                            p { class: "muted small", "snapshot from {human_time(at, now)}" }
                        }
                    }
                }
            }
        }
    }
}

/// Per-row delete / delete-and-repull buttons.
#[component]
fn RowActions(id: i64, label: String, on_done: EventHandler<()>) -> Element {
    let mut busy = use_signal(|| false);
    rsx! {
        button {
            class: "btn btn-sm",
            disabled: busy(),
            title: "Delete from cache and pull a fresh copy",
            onclick: move |_| {
                if busy() { return; }
                busy.set(true);
                spawn(async move {
                    let _ = api::send_empty::<()>("POST", &format!("/api/artifacts/{id}/repull"), None).await;
                    api::sleep_ms(800).await;
                    on_done.call(());
                    busy.set(false);
                });
            },
            if busy() { "…" } else { "⟳" }
        }
        button {
            class: "btn btn-sm btn-danger",
            disabled: busy(),
            title: "Delete from cache",
            onclick: move |_| {
                if busy() { return; }
                let confirmed = web_sys::window()
                    .and_then(|w| w.confirm_with_message(&format!("Delete {label} from the cache?")).ok())
                    .unwrap_or(false);
                if !confirmed { return; }
                busy.set(true);
                spawn(async move {
                    let _ = api::send_empty::<()>("DELETE", &format!("/api/artifacts/{id}"), None).await;
                    api::sleep_ms(500).await;
                    on_done.call(());
                    busy.set(false);
                });
            },
            "🗑"
        }
    }
}

/// Minimal query-string escaping for user input.
pub(super) fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
