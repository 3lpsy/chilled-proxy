//! Logger installation and the startup configuration log.

use std::sync::Arc;

use env_logger::{Builder as LogBuilder, Env as LogEnv};
use log::info;

use crate::cli::ResolvedConfig;
use crate::redact::redacted;

/// Initializes logging (stdout) at the resolved level. With the UI on, the
/// logger also tees into the /api/logs ring buffer; the hub is returned so the
/// UI runtime can read it.
pub(crate) fn init(config: &ResolvedConfig) -> Option<Arc<chilled_api::LogHub>> {
    let logger = LogBuilder::from_env(LogEnv::new().default_filter_or(config.log_level.as_str()))
        .target(env_logger::Target::Stdout)
        .build();
    let log_hub = config
        .ui
        .as_ref()
        .map(|_| Arc::new(chilled_api::LogHub::default()));
    let max_level = logger.filter();
    let boxed: Box<dyn log::Log> = match &log_hub {
        Some(hub) => Box::new(chilled_api::TeeLogger::new(logger, hub.clone())),
        None => Box::new(logger),
    };
    log::set_boxed_logger(boxed).expect("logger installed once");
    log::set_max_level(max_level);
    // A panic in a handler task otherwise dies silently with its connection;
    // route it through the logger so stdout and the UI logs both show it.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
    log_hub
}

/// Logs the effective configuration. Call *after* the logger is initialized.
pub(crate) fn log_startup(config: &ResolvedConfig) {
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
    match &config.ui {
        Some(ui) => info!(
            "ui: enabled at /ui (auth {:?}, public readonly {}, snapshot every {}s, db {})",
            ui.auth_mode,
            ui.public_readonly,
            ui.cache_update_interval.as_secs(),
            ui.db_path.to_string_lossy()
        ),
        None => info!("ui: disabled"),
    }
}
