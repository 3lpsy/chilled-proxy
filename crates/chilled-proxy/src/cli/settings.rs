//! Mount validation and per-instance settings resolution.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use chilled_core::config::{parse_overrides, RegistrySettings};
use url::Url;

use crate::cli::Cli;
use crate::kind::RegistryKind;
use crate::spec::MountSpec;

impl Cli {
    /// Validates the configuration (mount syntax, layout, auth) without
    /// keeping the result. Shorthand for [`Cli::resolve`] where only the
    /// validation verdict matters.
    pub fn check_mounts(&self) -> Result<(), String> {
        self.resolve().map(|_| ())
    }

    /// Resolves a registry's default-instance settings.
    #[cfg(test)]
    pub(crate) fn registry_settings(&self, kind: RegistryKind) -> RegistrySettings {
        self.settings_for(kind, kind.id(), self.mount_path(kind), None)
    }

    /// Resolves one instance's settings: a spec value wins over the registry
    /// flag, which wins over the general flag.
    pub(super) fn settings_for(
        &self,
        kind: RegistryKind,
        name: &str,
        path: &str,
        spec: Option<&MountSpec>,
    ) -> RegistrySettings {
        let (cooldown, ttl, overrides, restrict, proxy_url) = match kind {
            RegistryKind::Crates => (
                self.cooldown_crates,
                self.cache_ttl_crates,
                &self.cooldown_overrides_crates,
                self.restrict_downloads_crates,
                &self.crates_proxy_url,
            ),
            RegistryKind::Npm => (
                self.cooldown_npm,
                self.cache_ttl_npm,
                &self.cooldown_overrides_npm,
                self.restrict_downloads_npm,
                &self.npm_proxy_url,
            ),
            RegistryKind::Pypi => (
                self.cooldown_pypi,
                self.cache_ttl_pypi,
                &self.cooldown_overrides_pypi,
                self.restrict_downloads_pypi,
                &self.pypi_proxy_url,
            ),
            RegistryKind::Maven => (
                self.cooldown_maven,
                self.cache_ttl_maven,
                &self.cooldown_overrides_maven,
                self.restrict_downloads_maven,
                &self.maven_proxy_url,
            ),
        };

        // Size caps carry a *per-registry* built-in default (a 16 MiB crate and
        // a 512 MiB jar are both normal), so the fall-back chain ends at that
        // registry's own constant rather than at one shared general value.
        let (meta_registry, artifact_registry, meta_default, artifact_default) = match kind {
            RegistryKind::Crates => (
                self.max_metadata_size_crates,
                self.max_artifact_size_crates,
                crates_proxy::DEFAULT_MAX_METADATA_SIZE,
                crates_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
            ),
            RegistryKind::Npm => (
                self.max_metadata_size_npm,
                self.max_artifact_size_npm,
                npm_proxy::DEFAULT_MAX_METADATA_SIZE,
                npm_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
            ),
            RegistryKind::Pypi => (
                self.max_metadata_size_pypi,
                self.max_artifact_size_pypi,
                pypi_proxy::DEFAULT_MAX_METADATA_SIZE,
                pypi_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
            ),
            RegistryKind::Maven => (
                self.max_metadata_size_maven,
                self.max_artifact_size_maven,
                maven_proxy::DEFAULT_MAX_METADATA_SIZE,
                maven_proxy::DEFAULT_MAX_ARTIFACT_SIZE,
            ),
        };

        // Override *lists* are not settable per mount: the spec grammar spends
        // the comma on its own separator.
        let override_set: HashSet<String> = match overrides {
            Some(list) => parse_overrides(list),
            None => parse_overrides(&self.cooldown_overrides),
        };

        // `--<registry>-proxy-url` applies to the registry's default instance
        // only; any other mount states its own or derives it from its path.
        let proxy_url = match spec {
            Some(spec) => spec.proxy_url.clone(),
            None if name == kind.id() => proxy_url.clone(),
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
            max_metadata_size: spec
                .and_then(|s| s.max_metadata_size)
                .or(meta_registry)
                .or(self.max_metadata_size)
                .unwrap_or(meta_default),
            max_artifact_size: spec
                .and_then(|s| s.max_artifact_size)
                .or(artifact_registry)
                .or(self.max_artifact_size)
                .unwrap_or(artifact_default),
        }
    }

    /// Default external mount URL: the reverse-proxy base plus the mount path
    /// when one is configured, else derived from the listen address.
    fn default_proxy_url(&self, mount: &str) -> Url {
        if let Some(base) = &self.reverse_proxy_url {
            let base = ensure_trailing_slash(base);
            let mount = mount.trim_matches('/');
            if mount.is_empty() {
                return base;
            }
            return base
                .join(&format!("{mount}/"))
                .expect("valid reverse-proxy mount URL");
        }
        let host_port = match self.listen.rsplit_once(':') {
            // An all-interfaces bind has no routable host. Use the 127.0.0.1
            // literal, not "localhost": dual-stack clients resolve localhost
            // to ::1 first, which container port forwards accept and then
            // reset when the server side is IPv4-only.
            Some((h, p)) if h != "0.0.0.0" && h != "[::]" && h != "::" => {
                format!("{h}:{p}")
            }
            Some((_, p)) => format!("127.0.0.1:{p}"),
            None => "127.0.0.1:3080".to_string(),
        };
        let mount = mount.trim_end_matches('/');
        Url::parse(&format!("http://{host_port}{mount}/")).expect("valid derived proxy URL")
    }
}

/// Appends a trailing slash to a URL path if missing (relative joins need it).
pub(super) fn ensure_trailing_slash(url: &Url) -> Url {
    if url.path().ends_with('/') {
        return url.clone();
    }
    let mut u = url.clone();
    u.set_path(&format!("{}/", url.path()));
    u
}
