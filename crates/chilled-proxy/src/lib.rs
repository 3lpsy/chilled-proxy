//! chilled-proxy: one caching, cooldown-enforcing proxy for four registries.
//!
//! Each registry proxy (crates.io, npm, PyPI, Maven) is mounted under a path
//! prefix on a single listener; the top serves `/`, `/healthz`, `/metrics`.

pub(crate) mod auth;
pub mod cli;
pub(crate) mod constants;
pub mod kind;
pub(crate) mod logging;
pub(crate) mod mount;
pub(crate) mod redact;
pub(crate) mod registries;
pub(crate) mod routes;
pub(crate) mod spec;
pub(crate) mod ui_bridge;
pub(crate) mod version;

use std::sync::Arc;

use axum::{routing::get, Router};
use chilled_core::http::error_response;
use chilled_core::serve::serve;
use clap::Parser;

use crate::cli::{Cli, ResolvedConfig};
use crate::registries::build_registries;
use crate::routes::{handle_healthz, handle_home, handle_metrics, MountedRegistry, TopState};

/// Builds the full application router: every enabled registry nested under its
/// prefix, plus the top-level status surface. Infallible: the configuration
/// was already resolved and validated. UI-less: `run()` and the tests that
/// need /api and /ui go through [`build_full_app`].
pub fn build_app(config: &ResolvedConfig) -> Router {
    build_app_with(config, build_registries(&config.instances), None)
}

/// Builds the router plus the UI runtime when `--ui` is on. Fallible only
/// because UI startup touches the database.
pub async fn build_full_app(
    config: &ResolvedConfig,
) -> Result<(Router, Option<chilled_api::UiState>), String> {
    build_full_app_with_hub(config, None).await
}

/// [`build_full_app`], wiring an already-installed log tee into the UI state.
/// Also spawns the snapshot task, so `POST /api/snapshots/refresh` and the
/// interval work wherever the app is built (server and tests alike).
async fn build_full_app_with_hub(
    config: &ResolvedConfig,
    log_hub: Option<Arc<chilled_api::LogHub>>,
) -> Result<(Router, Option<chilled_api::UiState>), String> {
    let registries = build_registries(&config.instances);
    let ui = ui_bridge::startup(config, &registries, log_hub).await?;
    if let Some(ui_state) = &ui {
        chilled_api::spawn_snapshot_task(ui_state.clone());
    }
    Ok((build_app_with(config, registries, ui.clone()), ui))
}

/// The router over pre-built registries, with the UI merged in when enabled.
fn build_app_with(
    config: &ResolvedConfig,
    registries: Vec<MountedRegistry>,
    ui: Option<chilled_api::UiState>,
) -> Router {
    let state = TopState {
        registries: Arc::new(registries),
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
    // /api and /ui are reserved mount prefixes, so merging first cannot
    // collide; explicit routes also win over a root-mounted registry fallback.
    if let Some(ui_state) = ui {
        app = app.merge(chilled_api::ui_router(ui_state));
    }
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

    let log_hub = logging::init(&config);
    logging::log_startup(&config);

    // UI startup (database, migrations, bootstrap user) fails before binding,
    // like every other configuration problem.
    let (app, _ui) = match build_full_app_with_hub(&config, log_hub).await {
        Ok(built) => built,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };
    serve(config.listen, app).await;
}
