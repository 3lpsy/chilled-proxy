//! HTTP server bootstrap.

use axum::Router;
use log::info;

/// Server listening address.
pub enum ListenAddress {
    /// IP address + port.
    SocketAddr(String),
    /// Unix domain socket path.
    UnixPath(String),
}

/// Resolves when SIGINT (Ctrl+C) or SIGTERM arrives. Installing handlers also
/// makes the signals work as container PID 1, which discards unhandled ones.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("proxy: shutdown signal received, draining connections");
}

/// Serves `app` on an already-bound TCP listener until killed or signalled.
///
/// Exposed for embedding/tests: bind an ephemeral port, read `local_addr`, then
/// drive the router over real HTTP.
pub async fn serve_listener(listener: tokio::net::TcpListener, app: Router) {
    use axum::serve::ListenerExt;
    // Small responses (304s, index entries) should not sit in Nagle's buffer.
    let listener = listener.tap_io(|stream| {
        let _ = stream.set_nodelay(true);
    });
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("HTTP server error");
}

/// Binds the listener and serves until killed or signalled.
pub async fn serve(listen_addr: ListenAddress, app: Router) {
    match listen_addr {
        ListenAddress::SocketAddr(addr) => {
            info!("proxy: starting HTTP server at: {addr}");
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
            serve_listener(listener, app).await;
        }
        ListenAddress::UnixPath(path) => {
            info!("proxy: starting HTTP server at Unix socket {path}");
            // Reap a stale socket file before binding.
            std::fs::remove_file(&path).ok();
            let listener = tokio::net::UnixListener::bind(&path)
                .unwrap_or_else(|e| panic!("failed to bind {path}: {e}"));
            axum::serve(listener, app.into_make_service())
                .with_graceful_shutdown(shutdown_signal())
                .await
                .expect("HTTP server error");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    #[tokio::test]
    async fn serve_listener_answers_requests() {
        let app = Router::new().route("/ping", get(|| async { "pong" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_listener(listener, app));

        let body = reqwest::get(format!("http://{addr}/ping"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "pong");
    }
}
