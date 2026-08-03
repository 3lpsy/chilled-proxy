//! chilled-proxy: one caching, cooldown-enforcing proxy for four registries.
//!
//! Each registry proxy (crates.io, npm, PyPI, Maven) is mounted under a path
//! prefix on a single listener — `/crates`, `/npm`, `/pypi`, `/maven` by
//! default, configurable per registry. The top level serves `/` (status),
//! `/healthz`, and optionally `/metrics`.

pub mod cli;
pub(crate) mod constants;
pub(crate) mod mount;
pub(crate) mod routes;
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

use crate::cli::Cli;
use crate::constants::HTTP_USER_AGENT;
use crate::routes::{handle_healthz, handle_home, handle_metrics, TopState};

/// Builds the enabled registry proxies from the parsed CLI.
pub fn build_registries(cli: &Cli, client: &reqwest::Client) -> Vec<Arc<dyn RegistryProxy>> {
    let mut registries: Vec<Arc<dyn RegistryProxy>> = Vec::new();

    if !cli.disable_crates {
        let config = crates_proxy::Config::new(
            cli.crates_index_url.clone(),
            cli.crates_upstream_url.clone(),
            cli.registry_settings("crates"),
        );
        registries.push(Arc::new(crates_proxy::CratesProxy::new(
            config,
            client.clone(),
        )));
    }
    if !cli.disable_npm {
        let config =
            npm_proxy::Config::new(cli.npm_upstream_url.clone(), cli.registry_settings("npm"));
        registries.push(Arc::new(npm_proxy::NpmProxy::new(config, client.clone())));
    }
    if !cli.disable_pypi {
        let config = pypi_proxy::Config::new(
            cli.pypi_upstream_url.clone(),
            cli.pypi_files_url.clone(),
            cli.registry_settings("pypi"),
        );
        registries.push(Arc::new(pypi_proxy::PypiProxy::new(config, client.clone())));
    }
    if !cli.disable_maven {
        let config = maven_proxy::Config::new(
            cli.maven_upstream_url.clone(),
            cli.registry_settings("maven"),
        );
        registries.push(Arc::new(maven_proxy::MavenProxy::new(
            config,
            client.clone(),
        )));
    }

    registries
}

/// Builds the full application router: every enabled registry nested under its
/// prefix, plus the top-level status surface.
pub fn build_app(cli: &Cli) -> Router {
    let client = reqwest::Client::builder()
        .user_agent(HTTP_USER_AGENT)
        .connect_timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client");

    let registries = build_registries(cli, &client);

    let state = TopState {
        registries: Arc::new(registries),
    };

    let mut top = Router::new()
        .route("/", get(handle_home))
        .route("/healthz", get(handle_healthz));

    // The metrics endpoint is only routed when enabled; otherwise it 404s.
    if cli.enable_metrics {
        top = top.route("/metrics", get(handle_metrics));
    }

    // Registry routers carry their own state; apply ours before mounting them.
    let mut app = top.with_state(state.clone());
    let mut root_mounted = false;
    for registry in state.registries.iter() {
        let path = cli.mount_path(registry.id());
        if path == "/" {
            // axum refuses `nest("/")`; merging keeps the top-level routes and
            // hands everything else to the registry's own fallback.
            root_mounted = true;
            app = app.merge(registry.router());
        } else {
            app = app.nest(path, registry.router());
        }
    }

    // A root-mounted registry supplies the fallback; adding ours would collide.
    if root_mounted {
        app
    } else {
        app.fallback(|| async { error_response(404) })
    }
}

/// Logs the effective configuration. Call *after* the logger is initialized.
fn log_startup(cli: &Cli) {
    for id in ["crates", "npm", "pypi", "maven"] {
        let disabled = match id {
            "crates" => cli.disable_crates,
            "npm" => cli.disable_npm,
            "pypi" => cli.disable_pypi,
            _ => cli.disable_maven,
        };
        if disabled {
            info!("proxy: registry {id} disabled");
            continue;
        }
        let s = cli.registry_settings(id);
        info!(
            "proxy: registry {id} mounted at {} (proxy URL {}, cache {})",
            cli.mount_path(id),
            s.proxy_url,
            s.cache_dir.to_string_lossy()
        );
        if s.cooldown.as_secs() == 0 {
            info!("cooldown: {id}: age-gating disabled (pass-through)");
        } else {
            info!(
                "cooldown: {id}: hiding versions newer than {} seconds ({} override(s)){}",
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
        if cli.enable_metrics {
            "enabled"
        } else {
            "disabled"
        }
    );
}

/// The binary entry point: parse the environment + CLI, initialize logging, and
/// serve until the process is killed.
pub async fn run() {
    let cli = Cli::parse();

    if cli.version {
        version::version();
        return;
    }

    if let Err(err) = cli.check_mounts() {
        eprintln!("error: {err}");
        std::process::exit(2);
    }

    // Initialize logging (stdout) before emitting any configuration logs.
    LogBuilder::from_env(LogEnv::new().default_filter_or(cli.resolved_log_level().as_str()))
        .target(env_logger::Target::Stdout)
        .init();
    log_startup(&cli);

    let listen = cli.listen_address();
    let app = build_app(&cli);
    serve(listen, app).await;
}
