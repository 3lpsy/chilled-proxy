//! CLI/env parsing and per-registry settings resolution.
//!
//! General knobs (`--cooldown`, `--cache-ttl`, ...) apply to every registry;
//! per-registry variants (`--cooldown-npm`, ...) override them. Env vars use
//! the `CHILLED_*` prefix (flag name uppercased, dashes to underscores).

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use chilled_core::config::{normalize_log_level, parse_overrides, RegistrySettings};
use chilled_core::cooldown;
use chilled_core::serve::ListenAddress;
use clap::builder::BoolishValueParser;
use clap::Parser;
use url::Url;

use crate::auth::{self, UpstreamAuth};
use crate::constants::{
    DEFAULT_CACHE_DIR, DEFAULT_CACHE_TTL_SECS, DEFAULT_MOUNTS, LISTEN_ADDRESS, REGISTRY_IDS,
};
use crate::mount;
use crate::spec::{self, MountSpec};

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

/// One mounted registry instance: a registry kind served at a path, with its
/// own upstream and settings. A registry can be mounted more than once.
#[derive(Debug, Clone)]
pub struct RegistryInstance {
    /// Registry kind: `crates`, `npm`, `pypi`, or `maven`.
    pub kind: &'static str,
    /// Instance name — the `/metrics` key and cache subdirectory. Unique per
    /// process; the default instance of a registry is named after its kind.
    pub name: String,
    /// Mount path on this proxy.
    pub path: String,
    /// Primary upstream: crates.io downloads, the npm registry, the PyPI simple
    /// index, or the Maven repository.
    pub upstream: Url,
    /// The registry's second URL where it has one: the crates.io sparse index
    /// or the PyPI file host.
    pub secondary: Option<Url>,
    /// Cooldown, cache, and mount settings resolved for this instance.
    pub settings: RegistrySettings,
    /// Credentials and headers sent with this mount's upstream requests.
    pub auth: UpstreamAuth,
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

    /// The `--<registry>-mount` specs given for a registry.
    fn mount_specs(&self, id: &str) -> &[String] {
        match id {
            "crates" => &self.crates_mounts,
            "npm" => &self.npm_mounts,
            "pypi" => &self.pypi_mounts,
            "maven" => &self.maven_mounts,
            other => unreachable!("unknown registry id: {other}"),
        }
    }

    /// A registry's default upstream URLs: `(primary, secondary)`.
    fn default_upstreams(&self, id: &str) -> (Url, Option<Url>) {
        match id {
            "crates" => (
                self.crates_upstream_url.clone(),
                Some(self.crates_index_url.clone()),
            ),
            "npm" => (self.npm_upstream_url.clone(), None),
            "pypi" => (
                self.pypi_upstream_url.clone(),
                Some(self.pypi_files_url.clone()),
            ),
            "maven" => (self.maven_upstream_url.clone(), None),
            other => unreachable!("unknown registry id: {other}"),
        }
    }

    /// Every mount this process will serve: each enabled registry's default
    /// instance, followed by its extra `--<registry>-mount` instances.
    pub fn instances(&self) -> Result<Vec<RegistryInstance>, String> {
        let mut out: Vec<RegistryInstance> = Vec::new();

        for kind in REGISTRY_IDS {
            // Parsed up front: an explicit mount replaces the built-in default
            // of the same name rather than colliding with it.
            let specs = self
                .mount_specs(kind)
                .iter()
                .map(|raw| spec::parse(kind, raw))
                .collect::<Result<Vec<MountSpec>, String>>()?;

            if self.is_enabled(kind) {
                let (upstream, secondary) = self.default_upstreams(kind);
                let path = self.mount_path(kind).to_owned();
                let at_root = path == "/";
                out.push(RegistryInstance {
                    kind,
                    name: kind.to_owned(),
                    settings: self.settings_for(kind, kind, &path, None),
                    path,
                    upstream: ensure_trailing_slash(&upstream),
                    secondary: secondary.as_ref().map(ensure_trailing_slash),
                    auth: UpstreamAuth::default(),
                });

                // A registry at `/` owns the whole listener, so its built-ins
                // have nowhere to go — that layout stays single-mount.
                if !self.no_default_mounts && !at_root {
                    for (_, name, path, upstream) in
                        DEFAULT_MOUNTS.iter().filter(|(reg, ..)| *reg == kind)
                    {
                        if specs.iter().any(|s| s.name == *name) {
                            continue;
                        }
                        out.push(RegistryInstance {
                            kind,
                            name: (*name).to_owned(),
                            path: (*path).to_owned(),
                            upstream: Url::parse(upstream)
                                .expect("built-in mount upstreams are valid URLs"),
                            secondary: secondary.as_ref().map(ensure_trailing_slash),
                            settings: self.settings_for(kind, name, path, None),
                            auth: UpstreamAuth::default(),
                        });
                    }
                }
            }

            // Extra mounts stand on their own, so a registry can be disabled at
            // its default path and still be served from named mounts.
            for spec in specs {
                let path = match &spec.path {
                    Some(path) => path.clone(),
                    None => mount::parse(&format!("/{}", spec.name))?,
                };
                let (upstream, secondary) = self.default_upstreams(kind);
                let upstream = spec.upstream.clone().unwrap_or(upstream);
                let secondary = spec.secondary.clone().or(secondary);
                out.push(RegistryInstance {
                    kind,
                    settings: self.settings_for(kind, &spec.name, &path, Some(&spec)),
                    name: spec.name,
                    path,
                    upstream: ensure_trailing_slash(&upstream),
                    secondary: secondary.as_ref().map(ensure_trailing_slash),
                    auth: UpstreamAuth::default(),
                });
            }
        }

        for (index, instance) in out.iter().enumerate() {
            if let Some(prior) = out[..index].iter().find(|o| o.name == instance.name) {
                return Err(format!(
                    "mount name '{}' is used twice (by {} and {}); a name keys the cache \
                     directory and the /metrics report, so it must be unique",
                    instance.name, prior.kind, instance.kind
                ));
            }
        }

        self.attach_auth(&mut out, &|key| {
            // An empty value reads as unset, so a cleared variable does not
            // become an empty credential.
            std::env::var(key).ok().filter(|v| !v.is_empty())
        })?;

        Ok(out)
    }

    /// Resolves each mount's upstream credentials and headers, and refuses auth
    /// aimed at a mount that is not served.
    fn attach_auth(
        &self,
        instances: &mut [RegistryInstance],
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<(), String> {
        let mut basic: HashMap<String, (String, String)> = HashMap::new();
        for raw in &self.upstream_basic_auth {
            let (mount, credentials) = auth::parse_basic_spec(raw)?;
            if basic.insert(mount.clone(), credentials).is_some() {
                return Err(format!("--upstream-basic-auth names mount '{mount}' twice"));
            }
        }
        let mut headers: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for raw in &self.upstream_headers {
            let (mount, pair) = auth::parse_header_spec(raw)?;
            headers.entry(mount).or_default().push(pair);
        }

        // Auth aimed at a mount that is not served would leave the real one
        // unauthenticated, surfacing as an upstream 401 much later.
        let served: HashSet<&str> = instances.iter().map(|i| i.name.as_str()).collect();
        for mount in basic.keys().chain(headers.keys()) {
            if !served.contains(mount.as_str()) {
                return Err(format!(
                    "upstream auth names mount '{mount}', which is not served; mounted: {}",
                    served.iter().copied().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        self.check_auth_env(instances, env)?;

        for instance in instances.iter_mut() {
            instance.auth = auth::resolve(
                &instance.name,
                basic.get(&instance.name),
                headers.get(&instance.name).map_or(&[][..], Vec::as_slice),
                env,
            )?;
        }
        Ok(())
    }

    /// Refuses `CHILLED_<NAME>_*` auth variables whose mount is not served, and
    /// mount names that collide once folded into an env token.
    fn check_auth_env(
        &self,
        instances: &[RegistryInstance],
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<(), String> {
        let mut tokens: HashMap<String, &str> = HashMap::new();
        for instance in instances {
            let token = auth::env_token(&instance.name);
            if let Some(prior) = tokens.insert(token.clone(), &instance.name) {
                return Err(format!(
                    "mounts '{prior}' and '{}' both read CHILLED_{token}_* auth variables; \
                     rename one",
                    instance.name
                ));
            }
        }

        for (key, _) in std::env::vars() {
            // The global flag variables end in a matching suffix but name no mount.
            if key == "CHILLED_UPSTREAM_HEADERS" || key == "CHILLED_UPSTREAM_BASIC_AUTH" {
                continue;
            }
            let Some(rest) = key.strip_prefix("CHILLED_") else {
                continue;
            };
            // A suffix carries its own leading `_`, so what precedes it is the
            // mount token — and must be non-empty.
            let Some(suffix) = auth::ENV_SUFFIXES
                .iter()
                .find(|s| rest.len() > s.len() && rest.ends_with(**s))
            else {
                continue;
            };
            let token = &rest[..rest.len() - suffix.len()];
            // Only complain about a variable that is actually set.
            if env(&key).is_none() || tokens.contains_key(token) {
                continue;
            }
            return Err(format!(
                "{key} names mount token '{token}', which no mount reads; mounted: {}",
                tokens.values().copied().collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(())
    }

    /// Validates the mounts against each other (root exclusivity, duplicates,
    /// nesting, reserved endpoints) and the mount specs' own syntax. Call
    /// before building the router.
    pub fn check_mounts(&self) -> Result<(), String> {
        let instances = self.instances()?;
        let mounts: Vec<(&str, String)> = instances
            .iter()
            .map(|i| (i.name.as_str(), i.path.clone()))
            .collect();
        mount::check(&mounts)
    }

    /// Resolves a registry's default-instance settings.
    pub fn registry_settings(&self, id: &str) -> RegistrySettings {
        self.settings_for(id, id, self.mount_path(id), None)
    }

    /// Resolves one instance's settings: a spec value wins over the registry
    /// flag, which wins over the general flag.
    fn settings_for(
        &self,
        id: &str,
        name: &str,
        path: &str,
        spec: Option<&MountSpec>,
    ) -> RegistrySettings {
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

        // Override *lists* are not settable per mount: the spec grammar spends
        // the comma on its own separator.
        let override_set: HashSet<String> = match overrides {
            Some(list) => parse_overrides(list),
            None => parse_overrides(&self.cooldown_overrides),
        };

        // `--<registry>-proxy-url` names the registry's own mount, so it applies
        // to the default instance only; any other mount states its own or has it
        // derived from its path.
        let proxy_url = match spec {
            Some(spec) => spec.proxy_url.clone(),
            None if name == id => proxy_url.clone(),
            None => None,
        };

        RegistrySettings {
            cache_dir: Path::new(&self.cache_dir).join(name),
            cache_ttl: Duration::from_secs(
                spec.and_then(|s| s.cache_ttl)
                    .or(ttl)
                    .unwrap_or(self.cache_ttl),
            ),
            cooldown: spec
                .and_then(|s| s.cooldown)
                .or(cooldown)
                .unwrap_or(self.cooldown),
            overrides: std::sync::Arc::new(override_set),
            restrict_downloads: spec
                .and_then(|s| s.restrict_downloads)
                .or(restrict)
                .unwrap_or(self.restrict_downloads),
            proxy_url: proxy_url
                .map(|u| ensure_trailing_slash(&u))
                .unwrap_or_else(|| self.default_proxy_url(path)),
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
