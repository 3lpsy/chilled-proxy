//! Turning parsed flags into the set of mounted registry instances,
//! including each mount's upstream credentials.

use std::collections::{HashMap, HashSet};

use chilled_core::config::{normalize_log_level, RegistrySettings};
use chilled_core::serve::ListenAddress;
use url::Url;

use crate::auth::{self, UpstreamAuth};
use crate::cli::settings::ensure_trailing_slash;
use crate::cli::Cli;
use crate::constants::{DEFAULT_MOUNTS, REGISTRY_IDS};
use crate::mount;
use crate::spec::{self, MountSpec};

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
    /// Extra hosts a PyPI mount may fetch distribution files from, beyond its
    /// own index and file hosts. Empty for other registries.
    pub file_hosts: Vec<String>,
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

        self.attach_auth(&mut out, &|key| {
            // An empty value reads as unset, so a cleared variable does not
            // become an empty credential.
            std::env::var(key).ok().filter(|v| !v.is_empty())
        })?;

        Ok(out)
    }

    /// Resolves each mount's upstream credentials and headers, and refuses auth
    /// aimed at a mount that is not served.
    /// Extra file hosts for one mount: its own key, else the general PyPI flag.
    /// Only PyPI resolves files by host, so other registries get nothing.
    fn file_hosts_for(&self, kind: &str, spec: Option<&MountSpec>) -> Vec<String> {
        if kind != "pypi" {
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

    pub(super) fn attach_auth(
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
}
