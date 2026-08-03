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
