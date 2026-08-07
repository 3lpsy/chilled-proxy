//! User management flows and the logs SSE endpoint.

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use chilled_api::AuthMode;
use common::*;
use http_body_util::BodyExt;
use serde_json::json;

#[tokio::test]
async fn user_management_crud_and_self_delete_guard() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config_with_admin(&dir)).await;
    let cookie = login(&app).await;

    // Unauthenticated user listing stays gated (even data is sensitive).
    let res = send(&app, get("/api/users")).await;
    assert_status(&res, StatusCode::UNAUTHORIZED);

    // Create a second user.
    let res = send(
        &app,
        Request::post("/api/users")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"username": "bob", "password": "bobs-password"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::CREATED);
    let bob = body_json(res).await;
    let bob_id = bob["id"].as_i64().unwrap();

    // Duplicate username conflicts.
    let res = send(
        &app,
        Request::post("/api/users")
            .header(header::COOKIE, &cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"username": "bob", "password": "bobs-password"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);

    // List shows both.
    let users = body_json(send(&app, get_with_cookie("/api/users", &cookie)).await).await;
    assert_eq!(users.as_array().unwrap().len(), 2);

    // Self-delete forbidden; deleting bob works; deleting again 404s.
    let me = body_json(send(&app, get_with_cookie("/api/users/me", &cookie)).await).await;
    let my_id = me["id"].as_i64().unwrap();
    let del = |id: i64| {
        Request::delete(format!("/api/users/{id}"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap()
    };
    assert_status(&send(&app, del(my_id)).await, StatusCode::FORBIDDEN);
    assert_status(&send(&app, del(bob_id)).await, StatusCode::NO_CONTENT);
    assert_status(&send(&app, del(bob_id)).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn profile_update_checks_current_password_and_purges_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config_with_admin(&dir)).await;
    let cookie = login(&app).await;
    let other_session = login(&app).await;

    let patch = |body: serde_json::Value, cookie: &str| {
        Request::patch("/api/users/me")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };

    // Wrong current password rejected.
    let res = send(
        &app,
        patch(
            json!({"current_password": "nope", "new_password": "next-password"}),
            &cookie,
        ),
    )
    .await;
    assert_status(&res, StatusCode::UNAUTHORIZED);

    // Password change keeps this session, kills the other one.
    let res = send(
        &app,
        patch(
            json!({"current_password": "swordfish-1", "new_password": "next-password"}),
            &cookie,
        ),
    )
    .await;
    assert_status(&res, StatusCode::OK);
    let me = body_json(send(&app, get_with_cookie("/api/users/me", &cookie)).await).await;
    assert_eq!(me["username"], "admin");
    let res = send(&app, get_with_cookie("/api/users/me", &other_session)).await;
    assert_status(&res, StatusCode::UNAUTHORIZED);

    // Username change with the new password.
    let res = send(
        &app,
        patch(
            json!({"current_password": "next-password", "username": "root"}),
            &cookie,
        ),
    )
    .await;
    let me = body_json(res).await;
    assert_eq!(me["username"], "root");
}

#[tokio::test]
async fn oidc_users_cannot_edit_profile_or_be_created() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config(AuthMode::Oidc, &dir)).await;
    let with_id =
        |req: axum::http::request::Builder| req.header("x-auth-request-email", "dev@example.com");

    let res = send(
        &app,
        with_id(Request::patch("/api/users/me"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"current_password": "", "username": "new"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);

    let res = send(
        &app,
        with_id(Request::post("/api/users"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"username": "x", "password": "long-enough"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::CONFLICT);

    // Profile is still visible.
    let res = send(
        &app,
        with_id(Request::get("/api/users/me"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_status(&res, StatusCode::OK);
    let me = body_json(res).await;
    assert_eq!(me["auth_source"], "oidc");
}

/// oidc users belong to the identity provider — deletes are refused, so the
/// provisioning cache can never brick a live user.
#[tokio::test]
async fn oidc_users_cannot_be_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _state) = router(config(AuthMode::Oidc, &dir)).await;

    // Provision two users via the trusted header.
    for who in ["a@example.com", "b@example.com"] {
        let req = Request::get("/api/users/me")
            .header("x-auth-request-email", who)
            .body(Body::empty())
            .unwrap();
        assert_status(&send(&app, req).await, StatusCode::OK);
    }
    let req = Request::get("/api/users")
        .header("x-auth-request-email", "a@example.com")
        .body(Body::empty())
        .unwrap();
    let users = body_json(send(&app, req).await).await;
    let b_id = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "b@example.com")
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    let req = Request::delete(format!("/api/users/{b_id}"))
        .header("x-auth-request-email", "a@example.com")
        .body(Body::empty())
        .unwrap();
    assert_status(&send(&app, req).await, StatusCode::CONFLICT);

    // b keeps working.
    let req = Request::get("/api/users/me")
        .header("x-auth-request-email", "b@example.com")
        .body(Body::empty())
        .unwrap();
    assert_status(&send(&app, req).await, StatusCode::OK);
}

#[tokio::test]
async fn logs_sse_serves_backlog_and_is_gated() {
    let dir = tempfile::tempdir().unwrap();
    let (app, state) = router(config_with_admin(&dir)).await;

    state
        .log_hub
        .push("INFO", "chilled_core::cache", "warmed".into());
    state.log_hub.push("ERROR", "chilled_proxy", "boom".into());

    // Gated: no session → 401 even though it's a GET.
    let res = send(&app, get("/api/logs?follow=false")).await;
    assert_status(&res, StatusCode::UNAUTHORIZED);

    let cookie = login(&app).await;
    let res = send(&app, get_with_cookie("/api/logs?follow=false", &cookie)).await;
    assert_status(&res, StatusCode::OK);
    assert!(res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    let body =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(body.contains("warmed"), "{body}");
    assert!(body.contains("boom"), "{body}");

    // Min-level filter drops the info line.
    let res = send(
        &app,
        get_with_cookie("/api/logs?follow=false&level=error", &cookie),
    )
    .await;
    let body =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(!body.contains("warmed"), "{body}");
    assert!(body.contains("boom"), "{body}");
}
