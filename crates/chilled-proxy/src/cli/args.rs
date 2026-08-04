//! The command-line/env flag declarations.

use std::time::Duration;

use chilled_core::config;
use chilled_core::cooldown;
use clap::builder::BoolishValueParser;
use clap::Parser;
use url::Url;

use crate::constants::{DEFAULT_CACHE_DIR, DEFAULT_CACHE_TTL_SECS, LISTEN_ADDRESS};
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

    /// Cap on an upstream metadata document (suffixes k, m, g; default is
    /// per-registry).
    #[arg(long, env = "CHILLED_MAX_METADATA_SIZE", value_parser = config::parse_size)]
    pub max_metadata_size: Option<usize>,

    /// Cap on an upstream artifact download (suffixes k, m, g; default is
    /// per-registry). Bodies are buffered, so this is also a memory ceiling.
    #[arg(long, env = "CHILLED_MAX_ARTIFACT_SIZE", value_parser = config::parse_size)]
    pub max_artifact_size: Option<usize>,

    // Per-registry size caps (default: the general flag, else the registry's own).
    /// Metadata size cap for crates.io only.
    #[arg(long, env = "CHILLED_MAX_METADATA_SIZE_CRATES", value_parser = config::parse_size)]
    pub max_metadata_size_crates: Option<usize>,

    /// Metadata size cap for npm only.
    #[arg(long, env = "CHILLED_MAX_METADATA_SIZE_NPM", value_parser = config::parse_size)]
    pub max_metadata_size_npm: Option<usize>,

    /// Metadata size cap for PyPI only.
    #[arg(long, env = "CHILLED_MAX_METADATA_SIZE_PYPI", value_parser = config::parse_size)]
    pub max_metadata_size_pypi: Option<usize>,

    /// Metadata size cap for Maven only.
    #[arg(long, env = "CHILLED_MAX_METADATA_SIZE_MAVEN", value_parser = config::parse_size)]
    pub max_metadata_size_maven: Option<usize>,

    /// Artifact size cap for crates.io only.
    #[arg(long, env = "CHILLED_MAX_ARTIFACT_SIZE_CRATES", value_parser = config::parse_size)]
    pub max_artifact_size_crates: Option<usize>,

    /// Artifact size cap for npm only.
    #[arg(long, env = "CHILLED_MAX_ARTIFACT_SIZE_NPM", value_parser = config::parse_size)]
    pub max_artifact_size_npm: Option<usize>,

    /// Extra hosts PyPI mounts may fetch distribution files from, space-separated.
    #[arg(long, env = "CHILLED_PYPI_FILE_HOSTS")]
    pub pypi_file_hosts: Option<String>,

    /// Artifact size cap for PyPI only.
    #[arg(long, env = "CHILLED_MAX_ARTIFACT_SIZE_PYPI", value_parser = config::parse_size)]
    pub max_artifact_size_pypi: Option<usize>,

    /// Artifact size cap for Maven only.
    #[arg(long, env = "CHILLED_MAX_ARTIFACT_SIZE_MAVEN", value_parser = config::parse_size)]
    pub max_artifact_size_maven: Option<usize>,

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

    // Extra mounts of a registry, each with its own upstream. Repeat the flag,
    // or separate specs with `;` in the env var.
    /// Extra crates.io mount: `name=..[,path=..,index=..,upstream=..,cooldown=..]`.
    #[arg(
        long = "crates-mount",
        env = "CHILLED_CRATES_MOUNTS",
        value_delimiter = ';'
    )]
    pub crates_mounts: Vec<String>,

    /// Extra npm mount: `name=..[,path=..,upstream=..,cooldown=..]`.
    #[arg(long = "npm-mount", env = "CHILLED_NPM_MOUNTS", value_delimiter = ';')]
    pub npm_mounts: Vec<String>,

    /// Extra PyPI mount: `name=..[,path=..,upstream=..,files=..,cooldown=..]`.
    #[arg(
        long = "pypi-mount",
        env = "CHILLED_PYPI_MOUNTS",
        value_delimiter = ';'
    )]
    pub pypi_mounts: Vec<String>,

    /// Extra Maven mount: `name=..[,path=..,upstream=..,cooldown=..]`.
    #[arg(
        long = "maven-mount",
        env = "CHILLED_MAVEN_MOUNTS",
        value_delimiter = ';'
    )]
    pub maven_mounts: Vec<String>,

    /// Do not serve the built-in extra mounts (Gradle Plugin Portal, Google Maven).
    #[arg(long, env = "CHILLED_NO_DEFAULT_MOUNTS", value_parser = BoolishValueParser::new())]
    pub no_default_mounts: bool,

    // Upstream authentication, per mount. Prefer the per-mount env vars for
    // secrets: an argv value is visible to anything that can read `ps`.
    /// Upstream credentials for a mount: `<mount>=<user>:<password>` (repeatable).
    #[arg(
        long = "upstream-basic-auth",
        env = "CHILLED_UPSTREAM_BASIC_AUTH",
        value_delimiter = ';'
    )]
    pub upstream_basic_auth: Vec<String>,

    /// Extra upstream header for a mount: `<mount>=<header>: <value>` (repeatable).
    #[arg(
        long = "upstream-header",
        env = "CHILLED_UPSTREAM_HEADERS",
        value_delimiter = ';'
    )]
    pub upstream_headers: Vec<String>,
}
