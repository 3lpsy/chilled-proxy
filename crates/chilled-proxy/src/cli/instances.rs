//! Turning parsed flags into the set of mounted registry instances,
//! including each mount's upstream credentials.

use std::collections::{HashMap, HashSet};

use chilled_core::config::{normalize_log_level, RegistrySettings};
use chilled_core::serve::ListenAddress;
use url::Url;

use crate::auth::{self, EnvSource, ProcessEnv, UpstreamAuth};
use crate::cli::settings::ensure_trailing_slash;
use crate::cli::Cli;
use crate::constants::DEFAULT_MOUNTS;
use crate::kind::RegistryKind;
use crate::mount;
use crate::spec::{self, MountSpec};

/// One mounted registry instance: a registry kind served at a path, with its
/// own upstream and settings. A registry can be mounted more than once.
#[derive(Debug, Clone)]
pub struct RegistryInstance {
    /// Registry kind.
    pub kind: RegistryKind,
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
    /// Extra hosts a PyPI mount may fetch distribution files from, beyond its
    /// own index and file hosts. Empty for other registries.
    pub file_hosts: Vec<String>,
}

/// Fully resolved runtime configuration: everything `run()` and `build_app()`
/// need, computed **once** from the CLI and the process environment. After
/// this, no config re-derivation and no further env access happen.
pub struct ResolvedConfig {
    /// Every mount this process serves, in mount order, validated.
    pub instances: Vec<RegistryInstance>,
    /// Registries disabled at their default mount (for the startup log).
    pub disabled: Vec<RegistryKind>,
    /// Whether `/metrics` is served.
    pub enable_metrics: bool,
    /// Resolved log level.
    pub log_level: String,
    /// Where to listen.
    pub listen: ListenAddress,
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

    /// Resolves and validates the whole configuration: mount specs, instance
    /// settings, mount-path layout, and upstream auth (from the environment).
    pub fn resolve(&self) -> Result<ResolvedConfig, String> {
        let instances = self.instances(&ProcessEnv)?;
        let mounts: Vec<(&str, String)> = instances
            .iter()
            .map(|i| (i.name.as_str(), i.path.clone()))
            .collect();
        mount::check(&mounts)?;
        Ok(ResolvedConfig {
            instances,
            disabled: RegistryKind::ALL
                .into_iter()
                .filter(|kind| !self.is_enabled(*kind))
                .collect(),
            enable_metrics: self.enable_metrics,
            log_level: self.resolved_log_level(),
            listen: self.listen_address(),
        })
    }

    /// The path a registry is served under (already normalized by clap).
    pub fn mount_path(&self, kind: RegistryKind) -> &str {
        match kind {
            RegistryKind::Crates => &self.crates_path,
            RegistryKind::Npm => &self.npm_path,
            RegistryKind::Pypi => &self.pypi_path,
            RegistryKind::Maven => &self.maven_path,
        }
    }

    /// Whether a registry is served at its default mount. Disabling a registry
    /// also drops its built-in extra mounts (for Maven: `gradle-plugins` and
    /// `google-maven`) — `--disable-maven` means "no Java proxying" — while
    /// explicit `--<registry>-mount` instances are always served.
    pub fn is_enabled(&self, kind: RegistryKind) -> bool {
        !match kind {
            RegistryKind::Crates => self.disable_crates,
            RegistryKind::Npm => self.disable_npm,
            RegistryKind::Pypi => self.disable_pypi,
            RegistryKind::Maven => self.disable_maven,
        }
    }

    /// The `--<registry>-mount` specs given for a registry.
    fn mount_specs(&self, kind: RegistryKind) -> &[String] {
        match kind {
            RegistryKind::Crates => &self.crates_mounts,
            RegistryKind::Npm => &self.npm_mounts,
            RegistryKind::Pypi => &self.pypi_mounts,
            RegistryKind::Maven => &self.maven_mounts,
        }
    }

    /// A registry's default upstream URLs: `(primary, secondary)`.
    fn default_upstreams(&self, kind: RegistryKind) -> (Url, Option<Url>) {
        match kind {
            RegistryKind::Crates => (
                self.crates_upstream_url.clone(),
                Some(self.crates_index_url.clone()),
            ),
            RegistryKind::Npm => (self.npm_upstream_url.clone(), None),
            RegistryKind::Pypi => (
                self.pypi_upstream_url.clone(),
                Some(self.pypi_files_url.clone()),
            ),
            RegistryKind::Maven => (self.maven_upstream_url.clone(), None),
        }
    }

    /// Every mount this process will serve: each enabled registry's default
    /// instance, followed by its extra `--<registry>-mount` instances.
    pub(super) fn instances(&self, env: &dyn EnvSource) -> Result<Vec<RegistryInstance>, String> {
        let mut out: Vec<RegistryInstance> = Vec::new();

        for kind in RegistryKind::ALL {
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
                    name: kind.id().to_owned(),
                    settings: self.settings_for(kind, kind.id(), &path, None),
                    path,
                    upstream: ensure_trailing_slash(&upstream),
                    secondary: secondary.as_ref().map(ensure_trailing_slash),
                    auth: UpstreamAuth::default(),
                    file_hosts: self.file_hosts_for(kind, None),
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
                            file_hosts: self.file_hosts_for(kind, None),
                        });
                    }
                }
            }

            // Extra mounts stand on their own, so a registry disabled at its
            // default path can still be served from named mounts.
            for spec in specs {
                let path = match &spec.path {
                    Some(path) => path.clone(),
                    None => mount::parse(&format!("/{}", spec.name))?,
                };
                // A custom upstream must state the registry's second URL too:
                // silently inheriting the default would pair a private mirror
                // with the public index/file host.
                if let Some(key) = kind.secondary_key() {
                    if spec.upstream.is_some() && spec.secondary.is_none() {
                        return Err(format!(
                            "--{kind}-mount '{}' sets upstream= but not {key}=; without it the \
                             mount would silently use the default {key} URL, so state {key}= \
                             explicitly",
                            spec.name
                        ));
                    }
                }
                let (upstream, secondary) = self.default_upstreams(kind);
                let upstream = spec.upstream.clone().unwrap_or(upstream);
                let secondary = spec.secondary.clone().or(secondary);
                let file_hosts = self.file_hosts_for(kind, Some(&spec));
                out.push(RegistryInstance {
                    kind,
                    settings: self.settings_for(kind, &spec.name, &path, Some(&spec)),
                    name: spec.name,
                    path,
                    upstream: ensure_trailing_slash(&upstream),
                    secondary: secondary.as_ref().map(ensure_trailing_slash),
                    auth: UpstreamAuth::default(),
                    file_hosts,
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

        self.attach_auth(&mut out, env)?;

        Ok(out)
    }

    /// Extra file hosts for one mount: its own key, else the general PyPI flag.
    /// Only PyPI resolves files by host, so other registries get nothing.
    fn file_hosts_for(&self, kind: RegistryKind, spec: Option<&MountSpec>) -> Vec<String> {
        if kind != RegistryKind::Pypi {
            return Vec::new();
        }
        match spec.map(|s| &s.file_hosts) {
            Some(hosts) if !hosts.is_empty() => hosts.clone(),
            _ => self
                .pypi_file_hosts
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        }
    }

    /// Resolves each mount's upstream credentials and headers, and refuses auth
    /// aimed at a mount that is not served.
    pub(super) fn attach_auth(
        &self,
        instances: &mut [RegistryInstance],
        env: &dyn EnvSource,
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
        env: &dyn EnvSource,
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

        for key in env.keys() {
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
            if env.get(&key).is_none() || tokens.contains_key(token) {
                continue;
            }
            return Err(format!(
                "{key} names mount token '{token}', which no mount reads; mounted: {}",
                tokens.values().copied().collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(())
    }
}
