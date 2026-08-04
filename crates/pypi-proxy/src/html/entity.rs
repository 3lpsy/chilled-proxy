//! HTML entity expansion for attribute values and link text.

/// Expands the five XML entities plus numeric character references, leaving
/// anything unrecognized as literal text.
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        // A bare `&` is common in URLs; only treat a short, closed run as an
        // entity so one stray ampersand cannot swallow the rest of the value.
        let Some(semi) = tail.find(';').filter(|i| *i <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        match decode(&tail[1..semi]) {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Decodes one entity body (between `&` and `;`).
fn decode(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => entity
            .strip_prefix('#')
            .and_then(|n| match n.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => n.parse::<u32>().ok(),
            })
            .and_then(char::from_u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_named_and_numeric_entities() {
        assert_eq!(unescape("a&amp;b"), "a&b");
        assert_eq!(unescape("&lt;tag&gt;"), "<tag>");
        assert_eq!(unescape("&quot;q&quot;"), "\"q\"");
        assert_eq!(unescape("it&apos;s"), "it's");
        assert_eq!(unescape("&#39;"), "'");
        assert_eq!(unescape("&#x2B;"), "+");
    }

    #[test]
    fn leaves_plain_text_and_bare_ampersands_alone() {
        assert_eq!(unescape("torch-2.1.0+cpu.whl"), "torch-2.1.0+cpu.whl");
        // A query string is not an entity, and must survive intact.
        assert_eq!(unescape("?a=1&b=2"), "?a=1&b=2");
        assert_eq!(unescape("&notanentity;"), "&notanentity;");
        // An unterminated run must not consume the remainder.
        assert_eq!(unescape("a&b"), "a&b");
    }
}
