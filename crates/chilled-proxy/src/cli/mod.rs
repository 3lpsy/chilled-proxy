//! CLI/env parsing and per-registry settings resolution.
//!
//! General knobs (`--cooldown`, `--cache-ttl`, ...) apply to every registry;
//! per-registry variants (`--cooldown-npm`, ...) override them. Env vars use
//! the `CHILLED_*` prefix (flag name uppercased, dashes to underscores).

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use chilled_core::config::{normalize_log_level, parse_overrides, RegistrySettings};
use chilled_core::cooldown;
use chilled_core::serve::ListenAddress;
use clap::builder::BoolishValueParser;
use clap::Parser;
use url::Url;

use crate::constants::{DEFAULT_CACHE_DIR, DEFAULT_CACHE_TTL_SECS, LISTEN_ADDRESS, REGISTRY_IDS};
use crate::mount;

/// Command-line arguments (each also populated from its `CHILLED_*` env var).
#[derive(Parser, Debug)]
#[command(
    name = "chilled-proxy",
    about = "Multi-registry caching proxy (crates.io, npm, PyPI, Maven) with cooldown age-gating",
    disable_version_flag = true
)]
pub struct Cli {
    /// Raise the log level (-v debug, -vv trace).
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Print version and exit.
    #[arg(short = 'V', long)]
    pub version: bool,

    /// Log level: error|warn|info|debug|trace|off.
    #[arg(short, long, env = "CHILLED_LOG_LEVEL")]
    pub log_level: Option<String>,

    /// Expose cached artifacts at the /metrics endpoint.
    #[arg(short = 'm', long, env = "CHILLED_ENABLE_METRICS", value_parser = BoolishValueParser::new())]
    pub enable_metrics: bool,

    /// Unix domain socket path to listen at (takes precedence over --listen).
    #[arg(long, env = "CHILLED_LISTEN_UNIX")]
    pub listen_unix: Option<String>,

    /// Address and port to listen at.
    #[arg(short = 'L', long, env = "CHILLED_LISTEN", default_value = LISTEN_ADDRESS)]
    pub listen: String,

    /// Proxy cache directory (each registry gets a subdirectory).
    #[arg(short = 'C', long, env = "CHILLED_CACHE_DIR", default_value = DEFAULT_CACHE_DIR)]
    pub cache_dir: String,

    /// Metadata cache entry Time-to-Live, in seconds (all registries).
    #[arg(short = 'T', long, env = "CHILLED_CACHE_TTL", default_value_t = DEFAULT_CACHE_TTL_SECS)]
    pub cache_ttl: u64,

    /// Hide versions newer than this (0 = off; suffixes s, m, h, d, w).
    #[arg(
        short = 'K',
        long,
        env = "CHILLED_COOLDOWN",
        default_value = "0",
        value_parser = cooldown::parse_duration
    )]
    pub cooldown: Duration,

    /// Packages exempt from cooldown, comma-separated (all registries).
    #[arg(
        short = 'O',
        long,
        env = "CHILLED_COOLDOWN_OVERRIDES",
        default_value = ""
    )]
    pub cooldown_overrides: String,

    /// Also refuse to *download* artifacts newer than the cooldown.
    #[arg(long, env = "CHILLED_RESTRICT_DOWNLOADS", value_parser = BoolishValueParser::new())]
    pub restrict_downloads: bool,

    // Per-registry enable/disable (all enabled by default).
    /// Do not serve the crates.io registry.
    #[arg(long, env = "CHILLED_DISABLE_CRATES", value_parser = BoolishValueParser::new())]
    pub disable_crates: bool,

    /// Do not serve the npm registry.
    #[arg(long, env = "CHILLED_DISABLE_NPM", value_parser = BoolishValueParser::new())]
    pub disable_npm: bool,

    /// Do not serve the PyPI registry.
    #[arg(long, env = "CHILLED_DISABLE_PYPI", value_parser = BoolishValueParser::new())]
    pub disable_pypi: bool,

    /// Do not serve the Maven repository.
    #[arg(long, env = "CHILLED_DISABLE_MAVEN", value_parser = BoolishValueParser::new())]
    pub disable_maven: bool,

    // Per-registry cooldown overrides (default: --cooldown).
    /// Cooldown for crates.io only.
    #[arg(long, env = "CHILLED_COOLDOWN_CRATES", value_parser = cooldown::parse_duration)]
    pub cooldown_crates: Option<Duration>,

    /// Cooldown for npm only.
    #[arg(long, env = "CHILLED_COOLDOWN_NPM", value_parser = cooldown::parse_duration)]
    pub cooldown_npm: Option<Duration>,

    /// Cooldown for PyPI only.
    #[arg(long, env = "CHILLED_COOLDOWN_PYPI", value_parser = cooldown::parse_duration)]
    pub cooldown_pypi: Option<Duration>,

    /// Cooldown for Maven only.
    #[arg(long, env = "CHILLED_COOLDOWN_MAVEN", value_parser = cooldown::parse_duration)]
    pub cooldown_maven: Option<Duration>,

    // Per-registry cache-TTL overrides (default: --cache-ttl).
    /// Cache TTL (seconds) for crates.io only.
    #[arg(long, env = "CHILLED_CACHE_TTL_CRATES")]
    pub cache_ttl_crates: Option<u64>,

    /// Cache TTL (seconds) for npm only.
    #[arg(long, env = "CHILLED_CACHE_TTL_NPM")]
    pub cache_ttl_npm: Option<u64>,

    /// Cache TTL (seconds) for PyPI only.
    #[arg(long, env = "CHILLED_CACHE_TTL_PYPI")]
    pub cache_ttl_pypi: Option<u64>,

    /// Cache TTL (seconds) for Maven only.
    #[arg(long, env = "CHILLED_CACHE_TTL_MAVEN")]
    pub cache_ttl_maven: Option<u64>,

    // Per-registry cooldown-override lists (replace --cooldown-overrides).
    /// Cooldown-exempt packages for crates.io only.
    #[arg(long, env = "CHILLED_COOLDOWN_OVERRIDES_CRATES")]
    pub cooldown_overrides_crates: Option<String>,

    /// Cooldown-exempt packages for npm only.
    #[arg(long, env = "CHILLED_COOLDOWN_OVERRIDES_NPM")]
    pub cooldown_overrides_npm: Option<String>,

    /// Cooldown-exempt packages for PyPI only.
    #[arg(long, env = "CHILLED_COOLDOWN_OVERRIDES_PYPI")]
    pub cooldown_overrides_pypi: Option<String>,

    /// Cooldown-exempt packages for Maven only.
    #[arg(long, env = "CHILLED_COOLDOWN_OVERRIDES_MAVEN")]
    pub cooldown_overrides_maven: Option<String>,

    // Per-registry restrict-downloads overrides (default: --restrict-downloads).
    /// Restrict downloads for crates.io only (=false to opt out).
    #[arg(long, env = "CHILLED_RESTRICT_DOWNLOADS_CRATES", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub restrict_downloads_crates: Option<bool>,

    /// Restrict downloads for npm only (=false to opt out).
    #[arg(long, env = "CHILLED_RESTRICT_DOWNLOADS_NPM", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub restrict_downloads_npm: Option<bool>,

    /// Restrict downloads for PyPI only (=false to opt out).
    #[arg(long, env = "CHILLED_RESTRICT_DOWNLOADS_PYPI", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub restrict_downloads_pypi: Option<bool>,

    /// Restrict downloads for Maven only (=false to opt out).
    #[arg(long, env = "CHILLED_RESTRICT_DOWNLOADS_MAVEN", num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new())]
    pub restrict_downloads_maven: Option<bool>,

    // Per-registry mount paths. `/` is allowed only when one registry is enabled.
    /// Path the crates.io registry is served under.
    #[arg(long, env = "CHILLED_CRATES_PATH", default_value = "/crates", value_parser = mount::parse)]
    pub crates_path: String,

    /// Path the npm registry is served under.
    #[arg(long, env = "CHILLED_NPM_PATH", default_value = "/npm", value_parser = mount::parse)]
    pub npm_path: String,

    /// Path the PyPI registry is served under.
    #[arg(long, env = "CHILLED_PYPI_PATH", default_value = "/pypi", value_parser = mount::parse)]
    pub pypi_path: String,

    /// Path the Maven repository is served under.
    #[arg(long, env = "CHILLED_MAVEN_PATH", default_value = "/maven", value_parser = mount::parse)]
    pub maven_path: String,

    // Registry-specific upstream/proxy URLs.
    /// Upstream crates.io sparse-index URL.
    #[arg(long, env = "CHILLED_CRATES_INDEX_URL", default_value = crates_proxy::INDEX_CRATES_IO_URL)]
    pub crates_index_url: Url,

    /// Upstream crates.io download URL.
    #[arg(long, env = "CHILLED_CRATES_UPSTREAM_URL", default_value = crates_proxy::CRATES_IO_URL)]
    pub crates_upstream_url: Url,

    /// External URL of this proxy's /crates mount (default derived from --listen).
    #[arg(long, env = "CHILLED_CRATES_PROXY_URL")]
    pub crates_proxy_url: Option<Url>,

    /// Upstream npm registry URL.
    #[arg(long, env = "CHILLED_NPM_UPSTREAM_URL", default_value = npm_proxy::NPM_REGISTRY_URL)]
    pub npm_upstream_url: Url,

    /// External URL of this proxy's /npm mount (default derived from --listen).
    #[arg(long, env = "CHILLED_NPM_PROXY_URL")]
    pub npm_proxy_url: Option<Url>,

    /// Upstream PyPI simple-index URL.
    #[arg(long, env = "CHILLED_PYPI_UPSTREAM_URL", default_value = pypi_proxy::PYPI_SIMPLE_URL)]
    pub pypi_upstream_url: Url,

    /// Upstream PyPI file-hosting URL.
    #[arg(long, env = "CHILLED_PYPI_FILES_URL", default_value = pypi_proxy::PYPI_FILES_URL)]
    pub pypi_files_url: Url,

    /// External URL of this proxy's /pypi mount (default derived from --listen).
    #[arg(long, env = "CHILLED_PYPI_PROXY_URL")]
    pub pypi_proxy_url: Option<Url>,

    /// Upstream Maven repository URL.
    #[arg(long, env = "CHILLED_MAVEN_UPSTREAM_URL", default_value = maven_proxy::MAVEN_CENTRAL_URL)]
    pub maven_upstream_url: Url,

    /// External URL of this proxy's /maven mount (default derived from --listen).
    #[arg(long, env = "CHILLED_MAVEN_PROXY_URL")]
    pub maven_proxy_url: Option<Url>,
}

impl Cli {
    /// Resolved log level: `-v`/`-vv` win over `--log-level`, then `info`.
    /// (`RUST_LOG` still overrides everything when the logger is built.)
    pub fn resolved_log_level(&self) -> String {
        match self.verbose {
            0 => normalize_log_level(self.log_level.clone()),
            1 => "debug".to_string(),
            _ => "trace".to_string(),
        }
    }

    /// The listen address (`--listen-unix` takes precedence).
    pub fn listen_address(&self) -> ListenAddress {
        match &self.listen_unix {
            Some(path) => ListenAddress::UnixPath(path.clone()),
            None => ListenAddress::SocketAddr(self.listen.clone()),
        }
    }

    /// The path a registry is served under (already normalized by clap).
    pub fn mount_path(&self, id: &str) -> &str {
        match id {
            "crates" => &self.crates_path,
            "npm" => &self.npm_path,
            "pypi" => &self.pypi_path,
            "maven" => &self.maven_path,
            other => unreachable!("unknown registry id: {other}"),
        }
    }

    /// Whether a registry is served at all.
    pub fn is_enabled(&self, id: &str) -> bool {
        !match id {
            "crates" => self.disable_crates,
            "npm" => self.disable_npm,
            "pypi" => self.disable_pypi,
            "maven" => self.disable_maven,
            other => unreachable!("unknown registry id: {other}"),
        }
    }

    /// The enabled registries paired with their mounts, in mount order.
    pub fn mounts(&self) -> Vec<(&'static str, String)> {
        REGISTRY_IDS
            .iter()
            .filter(|id| self.is_enabled(id))
            .map(|id| (*id, self.mount_path(id).to_owned()))
            .collect()
    }

    /// Validates the mounts against each other (root exclusivity, duplicates,
    /// reserved endpoints). Call before building the router.
    pub fn check_mounts(&self) -> Result<(), String> {
        mount::check(&self.mounts())
    }

    /// Resolves one registry's settings from the general flags + its overrides.
    pub fn registry_settings(&self, id: &str) -> RegistrySettings {
        let (cooldown, ttl, overrides, restrict, proxy_url) = match id {
            "crates" => (
                self.cooldown_crates,
                self.cache_ttl_crates,
                &self.cooldown_overrides_crates,
                self.restrict_downloads_crates,
                &self.crates_proxy_url,
            ),
            "npm" => (
                self.cooldown_npm,
                self.cache_ttl_npm,
                &self.cooldown_overrides_npm,
                self.restrict_downloads_npm,
                &self.npm_proxy_url,
            ),
            "pypi" => (
                self.cooldown_pypi,
                self.cache_ttl_pypi,
                &self.cooldown_overrides_pypi,
                self.restrict_downloads_pypi,
                &self.pypi_proxy_url,
            ),
            "maven" => (
                self.cooldown_maven,
                self.cache_ttl_maven,
                &self.cooldown_overrides_maven,
                self.restrict_downloads_maven,
                &self.maven_proxy_url,
            ),
            other => unreachable!("unknown registry id: {other}"),
        };

        let override_set: HashSet<String> = match overrides {
            Some(list) => parse_overrides(list),
            None => parse_overrides(&self.cooldown_overrides),
        };

        RegistrySettings {
            cache_dir: Path::new(&self.cache_dir).join(id),
            cache_ttl: Duration::from_secs(ttl.unwrap_or(self.cache_ttl)),
            cooldown: cooldown.unwrap_or(self.cooldown),
            overrides: std::sync::Arc::new(override_set),
            restrict_downloads: restrict.unwrap_or(self.restrict_downloads),
            proxy_url: proxy_url
                .clone()
                .map(|u| ensure_trailing_slash(&u))
                .unwrap_or_else(|| self.default_proxy_url(self.mount_path(id))),
        }
    }

    /// Default external mount URL, derived from the listen address and mount.
    fn default_proxy_url(&self, mount: &str) -> Url {
        let host_port = match self.listen.rsplit_once(':') {
            // An all-interfaces bind has no routable host; default to localhost.
            Some((h, p)) if h != "0.0.0.0" && h != "[::]" && h != "::" => {
                format!("{h}:{p}")
            }
            Some((_, p)) => format!("localhost:{p}"),
            None => "localhost:3080".to_string(),
        };
        let mount = mount.trim_end_matches('/');
        Url::parse(&format!("http://{host_port}{mount}/")).expect("valid derived proxy URL")
    }
}

/// Appends a trailing slash to a URL path if missing (relative joins need it).
fn ensure_trailing_slash(url: &Url) -> Url {
    if url.path().ends_with('/') {
        return url.clone();
    }
    let mut u = url.clone();
    u.set_path(&format!("{}/", url.path()));
    u
}
