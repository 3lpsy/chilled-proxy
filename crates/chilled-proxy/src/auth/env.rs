//! Environment access for auth resolution.

/// The `CHILLED_<NAME>_*` suffixes this module reads, for typo detection.
pub(crate) const ENV_SUFFIXES: &[&str] =
    &["_BASIC_AUTH_USERNAME", "_BASIC_AUTH_PASSWORD", "_HEADERS"];

/// A source of environment variables, injectable so auth resolution and the
/// typo-detecting key scan are testable without mutating the process env.
pub(crate) trait EnvSource {
    /// The value of `key`. Empty values read as unset, so a cleared variable
    /// does not become an empty credential.
    fn get(&self, key: &str) -> Option<String>;
    /// Every variable name in the environment.
    fn keys(&self) -> Vec<String>;
}

/// The process environment.
pub(crate) struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn keys(&self) -> Vec<String> {
        std::env::vars().map(|(k, _)| k).collect()
    }
}

#[cfg(test)]
impl EnvSource for std::collections::HashMap<String, String> {
    fn get(&self, key: &str) -> Option<String> {
        std::collections::HashMap::get(self, key)
            .cloned()
            .filter(|v| !v.is_empty())
    }

    fn keys(&self) -> Vec<String> {
        self.keys().cloned().collect()
    }
}

/// The `CHILLED_<NAME>_` env-var token for a mount name: uppercased, with `-`
/// and `.` folded to `_`.
pub(crate) fn env_token(name: &str) -> String {
    name.to_ascii_uppercase().replace(['-', '.'], "_")
}
