//! A targeted `<a>` scanner for simple-index pages.
//!
//! A simple index is a flat list of anchors, so a focused scanner avoids an
//! HTML parser dependency. Tolerant: a malformed anchor is skipped, not fatal.

/// A parsed open tag: its attributes, the offset past `>`, and whether it
/// closed itself.
type OpenTag = (Vec<(String, String)>, usize, bool);

/// One parsed `<a>`: its attributes and its text content.
pub(crate) struct Anchor {
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) text: String,
}

impl Anchor {
    /// An attribute's value, matched case-insensitively.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Scans `body` for `<a …>text</a>` elements.
pub(crate) fn anchors(body: &str) -> Vec<Anchor> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while let Some(start) = find_tag_start(bytes, i) {
        let Some((attrs, after_tag, self_closing)) = parse_attrs(body, start + 2) else {
            i = start + 2;
            continue;
        };
        let mut j = after_tag;

        let text = if self_closing {
            String::new()
        } else {
            match find_close(bytes, j) {
                Some((text_end, after_close)) => {
                    let raw = &body[j..text_end];
                    j = after_close;
                    strip_tags(raw)
                }
                // Unclosed anchor: take the rest of the document and stop.
                None => {
                    let raw = &body[j..];
                    j = body.len();
                    strip_tags(raw)
                }
            }
        };

        out.push(Anchor { attrs, text });
        i = j;
    }
    out
}

/// Finds the next `<a` that opens an anchor (not `<abbr`, `<article`, …).
fn find_tag_start(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1].eq_ignore_ascii_case(&b'a') {
            match bytes.get(i + 2) {
                Some(c) if c.is_ascii_whitespace() || *c == b'>' || *c == b'/' => return Some(i),
                None => return None,
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Parses attributes from just after `<a`, returning them with the offset past
/// `>` and whether the tag closed itself.
fn parse_attrs(body: &str, from: usize) -> Option<OpenTag> {
    let bytes = body.as_bytes();
    let mut i = from;
    let mut attrs = Vec::new();
    let mut self_closing = false;

    loop {
        i = skip_space(bytes, i);
        match bytes.get(i) {
            None => return None,
            Some(b'>') => return Some((attrs, i + 1, self_closing)),
            Some(b'/') => {
                self_closing = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        if i == name_start {
            // No progress on an unexpected byte; skip it rather than spin.
            i += 1;
            continue;
        }
        let name = body[name_start..i].to_owned();

        i = skip_space(bytes, i);
        if bytes.get(i) != Some(&b'=') {
            // A valueless attribute, as `data-yanked` is written when bare.
            attrs.push((name, String::new()));
            continue;
        }
        i = skip_space(bytes, i + 1);

        let value = match bytes.get(i) {
            Some(q @ (b'"' | b'\'')) => {
                let quote = *q;
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                let value = body[start..i].to_owned();
                i = (i + 1).min(bytes.len());
                value
            }
            Some(_) => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                body[start..i].to_owned()
            }
            None => return None,
        };
        attrs.push((name, value));
    }
}

/// Advances past ASCII whitespace.
fn skip_space(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Finds `</a…>` from `from`, returning the text end and the offset past it.
fn find_close(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i + 3 < bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && bytes[i + 2].eq_ignore_ascii_case(&b'a')
            && (bytes[i + 3] == b'>' || bytes[i + 3].is_ascii_whitespace())
        {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j] != b'>' {
                j += 1;
            }
            return Some((i, (j + 1).min(bytes.len())));
        }
        i += 1;
    }
    None
}

/// Drops any nested markup from an anchor's inner text.
fn strip_tags(raw: &str) -> String {
    if !raw.contains('<') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
