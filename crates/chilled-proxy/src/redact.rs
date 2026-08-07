//! Credential redaction for logs and the management API.

use url::Url;

/// A URL safe to expose: userinfo credentials are masked, and so is every
/// query value — `?auth_token=SECRET` is a real upstream credential pattern.
pub(crate) fn redacted(url: &Url) -> String {
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    let has_query = url.query().is_some_and(|q| !q.is_empty());
    if !has_userinfo && !has_query {
        return url.to_string();
    }
    let mut safe = url.clone();
    if has_userinfo {
        let _ = safe.set_username("***");
        let _ = safe.set_password(url.password().map(|_| "***"));
    }
    if has_query {
        let masked: Vec<String> = url
            .query_pairs()
            .map(|(key, _)| format!("{key}=***"))
            .collect();
        safe.set_query(Some(&masked.join("&")));
    }
    safe.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_userinfo_but_keeps_plain_urls() {
        let plain = Url::parse("https://example.com/repo").unwrap();
        assert_eq!(redacted(&plain), "https://example.com/repo");

        let creds = Url::parse("https://user:secret@example.com/repo").unwrap();
        assert_eq!(redacted(&creds), "https://***:***@example.com/repo");
    }

    #[test]
    fn masks_query_values_but_keeps_keys() {
        let url = Url::parse("https://repo.example.com/npm/?auth_token=SECRET&v=2").unwrap();
        let safe = redacted(&url);
        assert!(!safe.contains("SECRET"), "{safe}");
        assert_eq!(safe, "https://repo.example.com/npm/?auth_token=***&v=***");
    }
}
