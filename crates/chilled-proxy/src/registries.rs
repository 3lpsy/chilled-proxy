//! Building the registry proxies behind each mount.

use std::sync::Arc;
use std::time::Duration;

use chilled_core::registry::RegistryProxy;

use crate::auth::UpstreamAuth;
use crate::cli::RegistryInstance;
use crate::constants::HTTP_USER_AGENT;
use crate::kind::RegistryKind;
use crate::routes::MountedRegistry;

/// Builds an upstream HTTP client carrying a mount's credentials and headers.
/// Auth lives in the client's default headers so every upstream request
/// carries it without the four registry crates knowing auth exists.
fn http_client(auth: &UpstreamAuth) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent(HTTP_USER_AGENT)
        .connect_timeout(Duration::from_secs(30))
        // Per-read, not per-request: a stalled upstream is dropped without
        // capping how long a large-but-progressing download may take.
        .read_timeout(Duration::from_secs(60));
    if !auth.is_empty() {
        builder = builder.default_headers(auth.headers().clone());
    }
    builder.build().expect("failed to build HTTP client")
}

/// Builds the registry proxy serving one mount.
fn build_registry(instance: &RegistryInstance, client: &reqwest::Client) -> Arc<dyn RegistryProxy> {
    let settings = instance.settings.clone();
    let upstream = instance.upstream.clone();
    // The CLI layer fills in each registry's second URL, so a missing one here
    // is a wiring bug rather than a configuration error.
    let second = |what: &str| {
        instance.secondary.clone().unwrap_or_else(|| {
            panic!(
                "{} mount '{}' has no {what} URL",
                instance.kind, instance.name
            )
        })
    };

    match instance.kind {
        RegistryKind::Crates => {
            let config = crates_proxy::Config::new(
                crates_proxy::Upstreams {
                    index: second("index"),
                    download: upstream,
                },
                settings,
            );
            Arc::new(crates_proxy::CratesProxy::new(config, client.clone()))
        }
        RegistryKind::Npm => {
            let config = npm_proxy::Config::new(upstream, settings);
            Arc::new(npm_proxy::NpmProxy::new(config, client.clone()))
        }
        RegistryKind::Pypi => {
            let config = pypi_proxy::Config::new(
                pypi_proxy::Upstreams {
                    simple: upstream,
                    files: second("files"),
                },
                settings,
                &instance.file_hosts,
            );
            Arc::new(pypi_proxy::PypiProxy::new(config, client.clone()))
        }
        RegistryKind::Maven => {
            let config = maven_proxy::Config::new(upstream, settings);
            Arc::new(maven_proxy::MavenProxy::new(config, client.clone()))
        }
    }
}

/// Builds a proxy for every mounted instance, in mount order. Mounts without
/// upstream auth share one client (and so one connection pool); each
/// authenticated mount gets its own, so credentials never cross mounts.
pub(crate) fn build_registries(instances: &[RegistryInstance]) -> Vec<MountedRegistry> {
    let shared = http_client(&UpstreamAuth::default());
    instances
        .iter()
        .map(|instance| {
            let client = if instance.auth.is_empty() {
                shared.clone()
            } else {
                http_client(&instance.auth)
            };
            MountedRegistry {
                name: instance.name.clone(),
                proxy: build_registry(instance, &client),
            }
        })
        .collect()
}
