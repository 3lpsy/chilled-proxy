//! PEP 503 HTML rendering of a (filtered, rewritten) PEP 691 project doc.

#[cfg(test)]
mod tests;

use serde_json::Value;

/// Escapes the five HTML-significant characters.
pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// Renders one file entry as a PEP 503 anchor line, or `None` when the entry
/// lacks the required fields.
fn render_anchor(file: &Value) -> Option<String> {
    let filename = file.get("filename")?.as_str()?;
    let url = file.get("url")?.as_str()?;

    let mut href = html_escape(url);
    if let Some(sha256) = file
        .get("hashes")
        .and_then(|h| h.get("sha256"))
        .and_then(Value::as_str)
    {
        href = format!("{href}#sha256={}", html_escape(sha256));
    }

    let mut attrs = String::new();
    if let Some(requires) = file.get("requires-python").and_then(Value::as_str) {
        attrs.push_str(&format!(
            " data-requires-python=\"{}\"",
            html_escape(requires)
        ));
    }
    match file.get("yanked") {
        Some(Value::Bool(true)) => attrs.push_str(" data-yanked=\"\""),
        Some(Value::String(reason)) => {
            attrs.push_str(&format!(" data-yanked=\"{}\"", html_escape(reason)));
        }
        _ => {}
    }

    Some(format!(
        "<a href=\"{href}\"{attrs}>{}</a><br/>\n",
        html_escape(filename)
    ))
}

/// Renders the PEP 503 project page from an already-rewritten PEP 691 doc.
pub(crate) fn render_html(doc: &Value, project: &str) -> String {
    let title = html_escape(project);
    let mut out = format!(
        "<!DOCTYPE html><html><head><meta name=\"pypi:repository-version\" content=\"1.0\">\
         <title>Links for {title}</title></head><body><h1>Links for {title}</h1>\n"
    );
    for file in doc
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(anchor) = render_anchor(file) {
            out.push_str(&anchor);
        }
    }
    out.push_str("</body></html>");
    out
}
