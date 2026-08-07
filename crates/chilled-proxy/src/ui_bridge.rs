//! Bridging the resolved configuration into the UI runtime.
//!
//! Redaction happens here, at the boundary: `chilled-api` only ever receives
//! masked URLs and value-free auth summaries.

use std::sync::Arc;

use chilled_api::{MountOps, MountView, ServerView, UiState};
use chilled_core::serve::ListenAddress;
use url::Url;

use crate::cli::ResolvedConfig;
use crate::constants::VERSION;
use crate::redact::redacted;
use crate::routes::MountedRegistry;

/// Top-level server facts for the config view.
fn server_view(config: &ResolvedConfig) -> ServerView {
    ServerView {
        listen: match &config.listen {
            ListenAddress::SocketAddr(addr) => addr.clone(),
            ListenAddress::UnixPath(path) => format!("unix:{path}"),
        },
        log_level: config.log_level.clone(),
        metrics_enabled: config.enable_metrics,
        disabled: config.disabled.iter().map(|k| k.id().to_owned()).collect(),
    }
}

/// Builds the pre-redacted mount projections for the API.
pub(crate) fn mount_views(config: &ResolvedConfig) -> Vec<MountView> {
    config
        .instances
        .iter()
        .map(|instance| MountView {
            name: instance.name.clone(),
            kind: instance.kind.id().to_owned(),
            path: instance.path.clone(),
            upstream: redacted(&instance.upstream),
            secondary: instance.secondary.as_ref().map(redacted),
            proxy_url: redacted_str(&instance.settings.proxy_url),
            cooldown_secs: instance.settings.cooldown.as_secs(),
            cache_ttl_secs: instance.settings.cache_ttl.as_secs(),
            restrict_downloads: instance.settings.restrict_downloads,
            auth: instance.auth.summary(),
        })
        .collect()
}

fn redacted_str(url: &Url) -> String {
    redacted(url)
}

/// One blocking cache-scan closure per mount, for the snapshot task.
pub(crate) fn mount_ops(registries: &[MountedRegistry]) -> Vec<(String, MountOps)> {
    registries
        .iter()
        .map(|mounted| {
            let proxy = mounted.proxy.clone();
            let scan_proxy = proxy.clone();
            let purge_proxy = proxy.clone();
            let clear_proxy = proxy.clone();
            // Repulls drive the mount's own router in-process, so the fetch
            // takes the exact caching path a client request would.
            let router = mounted.proxy.router();
            let ops = MountOps {
                scan: Arc::new(move || scan_proxy.cache_stats()),
                purge_artifact: Arc::new(move |name, version| {
                    purge_proxy.purge_artifact(name, version)
                }),
                purge_all: Arc::new(move || clear_proxy.purge_all()),
                repull: Arc::new(move |path| {
                    let router = router.clone();
                    Box::pin(async move {
                        use tower::ServiceExt;
                        let Ok(request) =
                            axum::http::Request::get(&path).body(axum::body::Body::empty())
                        else {
                            return false;
                        };
                        match router.oneshot(request).await {
                            Ok(response) => response.status().is_success(),
                            Err(_) => false,
                        }
                    })
                }),
            };
            (mounted.name.clone(), ops)
        })
        .collect()
}

/// Prepares the UI runtime: database directory, connection, migrations, and
/// the bootstrap user.
pub(crate) async fn startup(
    config: &ResolvedConfig,
    registries: &[MountedRegistry],
    log_hub: Option<Arc<chilled_api::LogHub>>,
) -> Result<Option<UiState>, String> {
    let Some(ui) = &config.ui else {
        return Ok(None);
    };
    if let Some(parent) = ui.db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "cannot create UI database directory {}: {e}",
                    parent.display()
                )
            })?;
        }
    }
    let state = chilled_api::startup(
        ui.clone(),
        VERSION.to_owned(),
        server_view(config),
        mount_views(config),
        mount_ops(registries),
        log_hub,
    )
    .await?;
    Ok(Some(state))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::Cli;

    #[test]
    fn mount_views_redact_upstream_credentials() {
        let cli = Cli::try_parse_from([
            "chilled-proxy",
            "--maven-mount",
            "name=corp,upstream=https://user:pw@repo.corp.example/m2/",
        ])
        .unwrap();
        let config = cli.resolve().unwrap();
        let views = mount_views(&config);
        let corp = views.iter().find(|v| v.name == "corp").unwrap();
        assert!(corp.upstream.contains("***"));
        assert!(!corp.upstream.contains("pw"));
        assert!(views.iter().all(|v| !v.upstream.contains("pw")));
    }
}
