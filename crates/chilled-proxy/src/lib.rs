//! chilled-proxy: one caching, cooldown-enforcing proxy for four registries.
//!
//! Each registry proxy (crates.io, npm, PyPI, Maven) is mounted under a path
//! prefix on a single listener — `/crates`, `/npm`, `/pypi`, `/maven` by
//! default, configurable per registry. The top level serves `/` (status),
//! `/healthz`, and optionally `/metrics`.

pub(crate) mod auth;
pub mod cli;
pub(crate) mod constants;
pub mod kind;
pub(crate) mod mount;
pub(crate) mod routes;
pub(crate) mod spec;
pub(crate) mod version;

use std::sync::Arc;
use std::time::Duration;

use axum::{routing::get, Router};
use chilled_core::http::error_response;
use chilled_core::registry::RegistryProxy;
use chilled_core::serve::serve;
use clap::Parser;
use env_logger::{Builder as LogBuilder, Env as LogEnv};
use log::info;
use url::Url;

use crate::auth::UpstreamAuth;
use crate::cli::{Cli, RegistryInstance, ResolvedConfig};
use crate::constants::HTTP_USER_AGENT;
use crate::kind::RegistryKind;
use crate::routes::{handle_healthz, handle_home, handle_metrics, MountedRegistry, TopState};

/// Builds an upstream HTTP client carrying a mount's credentials and headers.
///
/// Auth lives in the client's default headers rather than at each call site, so
/// every upstream request a registry makes carries it without the four registry
/// crates knowing auth exists.
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

/// Builds the full application router: every enabled registry nested under its
/// prefix, plus the top-level status surface. Infallible: the configuration
/// was already resolved and validated.
pub fn build_app(config: &ResolvedConfig) -> Router {
    let state = TopState {
        registries: Arc::new(build_registries(&config.instances)),
    };

    let mut top = Router::new()
        .route("/", get(handle_home))
        .route("/healthz", get(handle_healthz));

    // The metrics endpoint is only routed when enabled; otherwise it 404s.
    if config.enable_metrics {
        top = top.route("/metrics", get(handle_metrics));
    }

    // Registry routers carry their own state; apply ours before mounting them.
    let mut app = top.with_state(state.clone());
    let mut root_mounted = false;
    for (instance, mounted) in config.instances.iter().zip(state.registries.iter()) {
        if instance.path == "/" {
            // axum refuses `nest("/")`; merging keeps the top-level routes and
            // hands everything else to the registry's own fallback.
            root_mounted = true;
            app = app.merge(mounted.proxy.router());
        } else {
            app = app.nest(&instance.path, mounted.proxy.router());
        }
    }

    // A root-mounted registry supplies the fallback; adding ours would collide.
    if root_mounted {
        app
    } else {
        app.fallback(|| async { error_response(404) })
    }
}

/// A URL safe to log: credentials embedded in the userinfo are masked.
fn redacted(url: &Url) -> String {
    if url.username().is_empty() && url.password().is_none() {
        return url.to_string();
    }
    let mut safe = url.clone();
    let _ = safe.set_username("***");
    let _ = safe.set_password(url.password().map(|_| "***"));
    safe.to_string()
}

/// Logs the effective configuration. Call *after* the logger is initialized.
fn log_startup(config: &ResolvedConfig) {
    for kind in &config.disabled {
        info!("proxy: registry {kind} disabled at its default mount");
    }
    for instance in &config.instances {
        let name = &instance.name;
        let s = &instance.settings;
        info!(
            "proxy: {} mount '{name}' at {} (upstream {}, proxy URL {}, cache {})",
            instance.kind,
            instance.path,
            redacted(&instance.upstream),
            s.proxy_url,
            s.cache_dir.to_string_lossy()
        );
        // The secondary URL (sparse index / file host) is exactly the setting
        // that can silently resolve wrong, so make it visible.
        if let Some(secondary) = &instance.secondary {
            let what = instance.kind.secondary_key().unwrap_or("secondary");
            info!("proxy: {name}: {what} upstream {}", redacted(secondary));
        }
        if let Some(auth) = instance.auth.describe() {
            info!("proxy: {name}: upstream auth: {auth}");
        }
        if s.cooldown.as_secs() == 0 {
            info!("cooldown: {name}: age-gating disabled (pass-through)");
        } else {
            info!(
                "cooldown: {name}: hiding versions newer than {} seconds ({} override(s)){}",
                s.cooldown.as_secs(),
                s.overrides.len(),
                if s.restrict_downloads {
                    "; downloads restricted"
                } else {
                    ""
                }
            );
        }
    }
    info!(
        "metrics: /metrics endpoint {}",
        if config.enable_metrics {
            "enabled"
        } else {
            "disabled"
        }
    );
}

/// The binary entry point: parse the environment + CLI, resolve the
/// configuration once, initialize logging, and serve until the process is
/// killed.
pub async fn run() {
    let cli = Cli::parse();

    if cli.version {
        version::version();
        return;
    }

    // One resolution covers everything: mount-spec syntax, layout validation,
    // and upstream auth — a bad config is reported before binding.
    let config = match cli.resolve() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };

    // Initialize logging (stdout) before emitting any configuration logs.
    LogBuilder::from_env(LogEnv::new().default_filter_or(config.log_level.as_str()))
        .target(env_logger::Target::Stdout)
        .init();
    log_startup(&config);

    let app = build_app(&config);
    serve(config.listen, app).await;
}
