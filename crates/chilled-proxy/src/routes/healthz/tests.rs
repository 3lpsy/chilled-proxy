use super::*;

#[tokio::test]
async fn healthz_is_ok_text() {
    let resp = handle_healthz().await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
}
