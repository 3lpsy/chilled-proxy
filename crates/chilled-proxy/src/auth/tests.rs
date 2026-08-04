use super::*;
use std::collections::HashMap;

/// An env lookup over a fixed set of variables.
fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |key: &str| map.get(key).cloned()
}

/// The value of `header` as a string.
fn header_of(auth: &UpstreamAuth, header: &str) -> Option<String> {
    auth.headers()
        .get(header)
        .map(|v| v.to_str().unwrap().to_owned())
}

#[test]
fn nothing_configured_is_empty() {
    let auth = resolve("maven", None, &[], &env(&[])).unwrap();
    assert!(auth.is_empty());
    assert_eq!(auth.describe(), None);
}

#[test]
fn base64_matches_rfc_4648_vectors() {
    // Padding is the easy thing to get wrong, so cover every remainder.
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
    assert_eq!(base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    // High bytes must not sign-extend.
    assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
}

#[test]
fn basic_auth_encodes_an_authorization_header() {
    let auth = resolve(
        "maven",
        Some(&("alice".to_owned(), "s3cr3t".to_owned())),
        &[],
        &env(&[]),
    )
    .unwrap();
    // base64("alice:s3cr3t")
    assert_eq!(
        header_of(&auth, "authorization").as_deref(),
        Some("Basic YWxpY2U6czNjcjN0")
    );
    assert_eq!(auth.describe().as_deref(), Some("basic auth"));
}

#[test]
fn credentials_never_reach_debug_output() {
    // The Authorization value is marked sensitive, so a config dump or a panic
    // message cannot leak it.
    let auth = resolve(
        "maven",
        Some(&("alice".to_owned(), "s3cr3t".to_owned())),
        &[],
        &env(&[]),
    )
    .unwrap();
    let debug = format!("{auth:?}");
    assert!(!debug.contains("YWxpY2U6czNjcjN0"), "leaked: {debug}");
    assert!(!debug.contains("s3cr3t"), "leaked: {debug}");
    assert!(debug.contains("Sensitive"), "unexpected: {debug}");
}

#[test]
fn basic_auth_comes_from_the_env_pair() {
    let auth = resolve(
        "gradle-plugins",
        None,
        &[],
        &env(&[
            ("CHILLED_GRADLE_PLUGINS_BASIC_AUTH_USERNAME", "alice"),
            ("CHILLED_GRADLE_PLUGINS_BASIC_AUTH_PASSWORD", "s3cr3t"),
        ]),
    )
    .unwrap();
    assert_eq!(
        header_of(&auth, "authorization").as_deref(),
        Some("Basic YWxpY2U6czNjcjN0")
    );
}

#[test]
fn the_cli_pair_wins_over_the_env_pair() {
    let auth = resolve(
        "maven",
        Some(&("cli".to_owned(), "pass".to_owned())),
        &[],
        &env(&[
            ("CHILLED_MAVEN_BASIC_AUTH_USERNAME", "env"),
            ("CHILLED_MAVEN_BASIC_AUTH_PASSWORD", "pass"),
        ]),
    )
    .unwrap();
    assert_eq!(
        header_of(&auth, "authorization").as_deref(),
        Some(format!("Basic {}", base64(b"cli:pass"))).as_deref()
    );
}

#[test]
fn half_a_credential_pair_is_an_error() {
    // Silently sending nothing would surface as an upstream 401 much later.
    let err = resolve(
        "maven",
        None,
        &[],
        &env(&[("CHILLED_MAVEN_BASIC_AUTH_USERNAME", "alice")]),
    )
    .unwrap_err();
    assert!(err.contains("without"), "unexpected: {err}");

    let err = resolve(
        "maven",
        None,
        &[],
        &env(&[("CHILLED_MAVEN_BASIC_AUTH_PASSWORD", "s3cr3t")]),
    )
    .unwrap_err();
    assert!(err.contains("without"), "unexpected: {err}");
}

#[test]
fn a_password_may_contain_separators() {
    // Only the first ':' splits, so a password keeps its own.
    let (mount, (user, password)) = parse_basic_spec("maven=alice:p:a=ss").unwrap();
    assert_eq!(mount, "maven");
    assert_eq!(user, "alice");
    assert_eq!(password, "p:a=ss");
}

#[test]
fn custom_headers_are_attached() {
    let auth = resolve(
        "internal",
        None,
        &[
            ("X-Build".to_owned(), "ci".to_owned()),
            ("X-Team".to_owned(), "platform".to_owned()),
        ],
        &env(&[]),
    )
    .unwrap();
    assert_eq!(header_of(&auth, "x-build").as_deref(), Some("ci"));
    assert_eq!(header_of(&auth, "x-team").as_deref(), Some("platform"));
    assert_eq!(auth.describe().as_deref(), Some("2 custom header(s)"));
}

#[test]
fn headers_come_from_the_env_list() {
    let auth = resolve(
        "internal",
        None,
        &[],
        &env(&[("CHILLED_INTERNAL_HEADERS", "X-Build: ci; X-Team: platform;")]),
    )
    .unwrap();
    assert_eq!(header_of(&auth, "x-build").as_deref(), Some("ci"));
    assert_eq!(header_of(&auth, "x-team").as_deref(), Some("platform"));
}

#[test]
fn a_cli_header_replaces_the_env_one() {
    let auth = resolve(
        "internal",
        None,
        &[("X-Build".to_owned(), "override".to_owned())],
        &env(&[("CHILLED_INTERNAL_HEADERS", "X-Build: env; X-Keep: yes")]),
    )
    .unwrap();
    assert_eq!(header_of(&auth, "x-build").as_deref(), Some("override"));
    assert_eq!(header_of(&auth, "x-keep").as_deref(), Some("yes"));
}

#[test]
fn header_auth_works_without_basic_auth() {
    // Token-style upstreams take a bare header instead of credentials.
    let auth = resolve(
        "internal",
        None,
        &[("Authorization".to_owned(), "Bearer abc123".to_owned())],
        &env(&[]),
    )
    .unwrap();
    assert_eq!(
        header_of(&auth, "authorization").as_deref(),
        Some("Bearer abc123")
    );
    // It is a credential, so it is sensitive too.
    let debug = format!("{auth:?}");
    assert!(!debug.contains("abc123"), "leaked: {debug}");
}

#[test]
fn basic_auth_and_an_authorization_header_conflict() {
    let err = resolve(
        "internal",
        Some(&("alice".to_owned(), "s3cr3t".to_owned())),
        &[("Authorization".to_owned(), "Bearer abc".to_owned())],
        &env(&[]),
    )
    .unwrap_err();
    assert!(err.contains("both set"), "unexpected: {err}");
}

#[test]
fn invalid_headers_are_refused() {
    let err = resolve(
        "internal",
        None,
        &[("Not A Header".to_owned(), "x".to_owned())],
        &env(&[]),
    )
    .unwrap_err();
    assert!(err.contains("not a valid header name"), "unexpected: {err}");

    let err = resolve(
        "internal",
        None,
        &[("X-Build".to_owned(), "bad\nvalue".to_owned())],
        &env(&[]),
    )
    .unwrap_err();
    assert!(
        err.contains("not a valid header value"),
        "unexpected: {err}"
    );
}

#[test]
fn a_username_may_not_contain_a_colon() {
    // RFC 7617 has no way to encode it, and the upstream would split it wrong.
    let err = resolve(
        "maven",
        Some(&("al:ice".to_owned(), "s3cr3t".to_owned())),
        &[],
        &env(&[]),
    )
    .unwrap_err();
    assert!(err.contains("may not contain ':'"), "unexpected: {err}");
}

#[test]
fn env_tokens_fold_punctuation() {
    assert_eq!(env_token("maven"), "MAVEN");
    assert_eq!(env_token("gradle-plugins"), "GRADLE_PLUGINS");
    assert_eq!(env_token("corp.internal"), "CORP_INTERNAL");
}

#[test]
fn header_specs_parse() {
    let (mount, (header, value)) = parse_header_spec("internal=X-Build: ci").unwrap();
    assert_eq!(mount, "internal");
    assert_eq!(header, "X-Build");
    assert_eq!(value, "ci");

    // A value may contain colons and equals signs.
    let (_, (_, value)) = parse_header_spec("internal=X-Trace: a=b:c").unwrap();
    assert_eq!(value, "a=b:c");
}

#[test]
fn malformed_specs_are_refused() {
    for raw in ["maven", "maven=alice", "=alice:pass", "maven=:pass"] {
        assert!(
            parse_basic_spec(raw).is_err(),
            "{raw} should be refused as basic auth"
        );
    }
    for raw in [
        "internal",
        "internal=X-Build",
        "=X-Build: ci",
        "internal=: ci",
    ] {
        assert!(
            parse_header_spec(raw).is_err(),
            "{raw} should be refused as a header"
        );
    }
}
