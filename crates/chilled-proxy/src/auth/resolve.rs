//! Resolving one mount's upstream auth from CLI values and the environment.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};

use super::env::{env_token, EnvSource};
use super::upstream::UpstreamAuth;

/// Resolves one mount's auth. `env` looks up an environment variable; the CLI
/// values win over it.
pub(crate) fn resolve(
    name: &str,
    cli_basic: Option<&(String, String)>,
    cli_headers: &[(String, String)],
    env: &dyn EnvSource,
) -> Result<UpstreamAuth, String> {
    let token = env_token(name);
    let err = |msg: String| format!("mount '{name}': {msg}");

    let mut headers = HeaderMap::new();

    // Env headers first, so a --upstream-header for the same header replaces
    // rather than duplicates.
    if let Some(list) = env.get(&format!("CHILLED_{token}_HEADERS")) {
        for (header, value) in parse_header_list(&list).map_err(err)? {
            insert(&mut headers, &header, &value).map_err(err)?;
        }
    }
    for (header, value) in cli_headers {
        insert(&mut headers, header, value).map_err(err)?;
    }
    let explicit_authorization = headers.contains_key(AUTHORIZATION);

    // Credentials: the CLI pair, else both env vars together.
    let env_user = env.get(&format!("CHILLED_{token}_BASIC_AUTH_USERNAME"));
    let env_pass = env.get(&format!("CHILLED_{token}_BASIC_AUTH_PASSWORD"));
    let credentials = match (cli_basic, env_user, env_pass) {
        (Some(pair), ..) => Some(pair.clone()),
        (None, Some(user), Some(password)) => Some((user, password)),
        // One half alone is a mistake worth reporting: silently sending no
        // credentials would look like an upstream permission problem.
        (None, Some(_), None) => {
            return Err(err(format!(
                "CHILLED_{token}_BASIC_AUTH_USERNAME is set without \
                 CHILLED_{token}_BASIC_AUTH_PASSWORD"
            )))
        }
        (None, None, Some(_)) => {
            return Err(err(format!(
                "CHILLED_{token}_BASIC_AUTH_PASSWORD is set without \
                 CHILLED_{token}_BASIC_AUTH_USERNAME"
            )))
        }
        (None, None, None) => None,
    };

    let basic = credentials.is_some();
    if let Some((user, password)) = credentials {
        if explicit_authorization {
            return Err(err(
                "basic auth and an explicit Authorization header are both set".to_owned(),
            ));
        }
        if user.contains(':') {
            return Err(err("a basic-auth username may not contain ':'".to_owned()));
        }
        let encoded = base64(format!("{user}:{password}").as_bytes());
        let mut value = HeaderValue::try_from(format!("Basic {encoded}"))
            .map_err(|_| err("credentials contain characters a header cannot carry".to_owned()))?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }

    Ok(UpstreamAuth { headers, basic })
}

/// Validates and inserts one header, replacing any earlier entry of that name.
fn insert(headers: &mut HeaderMap, header: &str, value: &str) -> Result<(), String> {
    let name = HeaderName::try_from(header)
        .map_err(|_| format!("'{header}' is not a valid header name"))?;
    let mut value = HeaderValue::try_from(value)
        .map_err(|_| format!("the value for '{header}' is not a valid header value"))?;
    // Anything that carries a credential should stay out of Debug output.
    if name == AUTHORIZATION || name.as_str().contains("token") || name.as_str().contains("key") {
        value.set_sensitive(true);
    }
    headers.insert(name, value);
    Ok(())
}

/// Parses a `--upstream-basic-auth` value: `<mount>=<user>:<password>`.
pub(crate) fn parse_basic_spec(raw: &str) -> Result<(String, (String, String)), String> {
    let err = || format!("--upstream-basic-auth '{raw}': expected <mount>=<user>:<password>");
    let (mount, credentials) = raw.split_once('=').ok_or_else(err)?;
    // Split at the first ':' only: a password may contain more.
    let (user, password) = credentials.split_once(':').ok_or_else(err)?;
    let mount = mount.trim();
    if mount.is_empty() || user.trim().is_empty() {
        return Err(err());
    }
    Ok((
        mount.to_owned(),
        (user.trim().to_owned(), password.to_owned()),
    ))
}

/// Parses an `--upstream-header` value: `<mount>=<header>: <value>`.
pub(crate) fn parse_header_spec(raw: &str) -> Result<(String, (String, String)), String> {
    let err = || format!("--upstream-header '{raw}': expected <mount>=<header>: <value>");
    let (mount, pair) = raw.split_once('=').ok_or_else(err)?;
    let (header, value) = split_header(pair).ok_or_else(err)?;
    let mount = mount.trim();
    if mount.is_empty() {
        return Err(err());
    }
    Ok((mount.to_owned(), (header, value)))
}

/// Parses a `CHILLED_<NAME>_HEADERS` list: `Header: value; Other: value`.
fn parse_header_list(raw: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for entry in raw.split(';') {
        let entry = entry.trim();
        // Tolerate a trailing separator.
        if entry.is_empty() {
            continue;
        }
        let (header, value) = split_header(entry)
            .ok_or_else(|| format!("header '{entry}' is not '<header>: <value>'"))?;
        out.push((header, value));
    }
    Ok(out)
}

/// Splits `Header: value` at the first colon, trimming one leading space off
/// the value as the wire format writes it.
fn split_header(raw: &str) -> Option<(String, String)> {
    let (header, value) = raw.split_once(':')?;
    let header = header.trim();
    if header.is_empty() {
        return None;
    }
    Some((header.to_owned(), value.trim_start().to_owned()))
}

/// Standard base64 alphabet, for the basic-auth credential encoding.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes `input` as padded standard base64 (RFC 4648).
pub(crate) fn base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        out.push(B64[(n >> 18 & 63) as usize] as char);
        out.push(B64[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
