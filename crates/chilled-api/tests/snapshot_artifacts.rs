//! Snapshot upsert/prune behavior and the paginated artifacts API.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use chilled_api::Scanner;
use chilled_core::registry::{CacheStats, CachedArtifact};
use common::*;

fn artifact(name: &str, version: &str, size: u64) -> CachedArtifact {
    CachedArtifact {
        name: name.into(),
        version: version.into(),
        cached_at: 1_700_000_000,
        size_bytes: size,
    }
}

/// A scanner whose second run drops lodash 4.17.20 and adds react.
fn evolving_scanner() -> Scanner {
    let runs = Arc::new(AtomicUsize::new(0));
    Arc::new(move || {
        let run = runs.fetch_add(1, Ordering::SeqCst);
        let mut artifacts = vec![
            artifact("lodash", "4.17.21", 100),
            artifact("@scope/pkg", "1.0.0", 50),
        ];
        if run == 0 {
            artifacts.push(artifact("lodash", "4.17.20", 90));
        } else {
            artifacts.push(artifact("react", "19.0.0", 200));
        }
        CacheStats {
            artifacts,
            incomplete: false,
        }
    })
}

#[tokio::test]
async fn snapshot_upserts_then_prunes_evicted_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let (app, state) =
        router_with_scanners(cfg, vec![("npm".to_string(), evolving_scanner())]).await;

    let count = chilled_api::snapshot::run_once(&state).await.unwrap();
    assert_eq!(count, 3);

    let res = send(&app, get("/api/artifacts?sort=name")).await;
    assert_status(&res, StatusCode::OK);
    let page = body_json(res).await;
    assert_eq!(page["total"], 3);
    assert_eq!(page["items"][0]["name"], "@scope/pkg");
    assert_eq!(page["items"][0]["size_bytes"], 50);
    assert_eq!(page["items"][0]["upstream"], "https://registry.npmjs.org/");
    assert_eq!(page["snapshot"]["artifact_count"], 3);

    // Second run: 4.17.20 evicted, react added; total stays 3, rows differ.
    chilled_api::snapshot::run_once(&state).await.unwrap();
    let page = body_json(send(&app, get("/api/artifacts?sort=name&order=desc")).await).await;
    assert_eq!(page["total"], 3);
    let names: Vec<String> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            format!(
                "{}@{}",
                i["name"].as_str().unwrap(),
                i["version"].as_str().unwrap()
            )
        })
        .collect();
    assert!(names.contains(&"react@19.0.0".to_string()));
    assert!(!names.contains(&"lodash@4.17.20".to_string()));

    // Registries totals reflect the snapshot.
    let regs = body_json(send(&app, get("/api/registries")).await).await;
    assert_eq!(regs[0]["artifact_count"], 3);
    assert_eq!(regs[0]["total_size_bytes"], 350);
    assert!(regs[0]["last_snapshot_at"].as_i64().unwrap() > 0);
}

/// A failed scan must not read as "cache emptied": rows survive and the next
/// good scan resumes normal pruning.
#[tokio::test]
async fn incomplete_scan_keeps_previous_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let fail_next = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = fail_next.clone();
    let scanner: Scanner = Arc::new(move || {
        if flag.load(Ordering::SeqCst) {
            CacheStats {
                incomplete: true,
                ..Default::default()
            }
        } else {
            CacheStats {
                artifacts: vec![artifact("lodash", "4.17.21", 10)],
                incomplete: false,
            }
        }
    });
    let (app, state) = router_with_scanners(cfg, vec![("npm".to_string(), scanner)]).await;

    chilled_api::snapshot::run_once(&state).await.unwrap();
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 1);

    // Scan failure: the row must survive.
    fail_next.store(true, Ordering::SeqCst);
    chilled_api::snapshot::run_once(&state).await.unwrap();
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 1, "failed scan must not prune");

    // Recovery: normal pruning applies again.
    fail_next.store(false, Ordering::SeqCst);
    chilled_api::snapshot::run_once(&state).await.unwrap();
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 1);
}

/// A scoped run rescans one mount and leaves the others' rows untouched.
#[tokio::test]
async fn scoped_run_touches_only_its_mount() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let npm_gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = npm_gone.clone();
    let npm_scanner: Scanner = Arc::new(move || CacheStats {
        artifacts: if flag.load(Ordering::SeqCst) {
            vec![]
        } else {
            vec![artifact("lodash", "4.17.21", 10)]
        },
        incomplete: false,
    });
    let crates_scanner: Scanner = Arc::new(|| CacheStats {
        artifacts: vec![artifact("serde", "1.0.0", 20)],
        incomplete: false,
    });
    let (app, state) = router_with_scanners(
        cfg,
        vec![
            ("npm".to_string(), npm_scanner),
            ("crates".to_string(), crates_scanner),
        ],
    )
    .await;

    chilled_api::snapshot::run_once(&state).await.unwrap();
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 2);

    // npm's cache empties; a scoped npm run prunes npm only.
    npm_gone.store(true, Ordering::SeqCst);
    let count = chilled_api::snapshot::run_mount(&state, "npm")
        .await
        .unwrap();
    assert_eq!(count, 0);
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 1, "crates row must survive a scoped npm run");
    assert_eq!(page["items"][0]["mount"], "crates");
}

/// Delete and clear endpoints purge files (via ops), drop rows, and are
/// write-protected.
#[tokio::test]
async fn delete_and_clear_purge_rows() {
    use std::sync::atomic::AtomicUsize;

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let purged = Arc::new(AtomicUsize::new(0));
    let cleared = Arc::new(AtomicUsize::new(0));
    let scan: chilled_api::Scanner = Arc::new(|| CacheStats {
        artifacts: vec![
            artifact("lodash", "4.17.21", 10),
            artifact("react", "19.0.0", 20),
        ],
        incomplete: false,
    });
    let p = purged.clone();
    let c = cleared.clone();
    let ops = chilled_api::MountOps {
        scan,
        purge_artifact: Arc::new(move |name, version| {
            p.fetch_add(1, Ordering::SeqCst);
            vec![format!("/{name}/-/{name}-{version}.tgz")]
        }),
        purge_all: Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }),
        repull: Arc::new(|_| Box::pin(async { true })),
    };
    let (app, state) = router_with_ops(cfg, vec![("npm".to_string(), ops)]).await;
    chilled_api::snapshot::run_once(&state).await.unwrap();

    let page = body_json(send(&app, get("/api/artifacts?sort=name")).await).await;
    assert_eq!(page["total"], 2);
    let id = page["items"][0]["id"].as_i64().unwrap();

    // Write-protected even in public-readonly mode.
    let anon = axum::http::Request::delete(format!("/api/artifacts/{id}"))
        .body(axum::body::Body::empty())
        .unwrap();
    assert_status(&send(&app, anon).await, StatusCode::UNAUTHORIZED);

    let cookie = login(&app).await;
    let del = axum::http::Request::delete(format!("/api/artifacts/{id}"))
        .header(axum::http::header::COOKIE, &cookie)
        .body(axum::body::Body::empty())
        .unwrap();
    let res = send(&app, del).await;
    assert_status(&res, StatusCode::OK);
    assert_eq!(body_json(res).await["removed_files"], 1);
    assert_eq!(purged.load(Ordering::SeqCst), 1);
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 1, "deleted row gone immediately");

    // Clear drops the mount's remaining rows and calls purge_all.
    let clear = axum::http::Request::post("/api/registries/npm/clear")
        .header(axum::http::header::COOKIE, &cookie)
        .body(axum::body::Body::empty())
        .unwrap();
    assert_status(&send(&app, clear).await, StatusCode::ACCEPTED);
    assert_eq!(cleared.load(Ordering::SeqCst), 1);
    let page = body_json(send(&app, get("/api/artifacts")).await).await;
    assert_eq!(page["total"], 0);
}

/// Repull purges then re-fetches through the mount's own routes.
#[tokio::test]
async fn repull_refetches_each_purged_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_with_admin(&dir);
    let fetched: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let sink = fetched.clone();
    let scan: chilled_api::Scanner = Arc::new(|| CacheStats {
        artifacts: vec![artifact("lodash", "4.17.21", 10)],
        incomplete: false,
    });
    let ops = chilled_api::MountOps {
        scan,
        purge_artifact: Arc::new(|name, version| vec![format!("/{name}/-/{name}-{version}.tgz")]),
        purge_all: Arc::new(|| {}),
        repull: Arc::new(move |path| {
            sink.lock().unwrap().push(path);
            Box::pin(async { true })
        }),
    };
    let (app, state) = router_with_ops(cfg, vec![("npm".to_string(), ops)]).await;
    chilled_api::snapshot::run_once(&state).await.unwrap();
    let cookie = login(&app).await;

    let page = body_json(send(&app, get_with_cookie("/api/artifacts", &cookie)).await).await;
    let id = page["items"][0]["id"].as_i64().unwrap();
    let req = axum::http::Request::post(format!("/api/artifacts/{id}/repull"))
        .header(axum::http::header::COOKIE, &cookie)
        .body(axum::body::Body::empty())
        .unwrap();
    let res = send(&app, req).await;
    assert_status(&res, StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["refetched"], 1);
    assert_eq!(body["failed"], 0);
    assert_eq!(
        fetched.lock().unwrap().as_slice(),
        ["/lodash/-/lodash-4.17.21.tgz"]
    );
}

/// The retention sweep bounds snapshot_runs and drops expired sessions.
#[tokio::test]
async fn retention_bounds_runs_and_sessions() {
    use sea_orm::{ActiveValue, EntityTrait, PaginatorTrait};

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let scanner: Scanner = Arc::new(CacheStats::default);
    let (_app, state) = router_with_scanners(cfg, vec![("npm".to_string(), scanner)]).await;

    // An expired session row that only the sweep will remove.
    use chilled_api::db::entity::{session, snapshot_run};
    let uid = chilled_api::db::entity::user::Entity::find()
        .one(&state.db)
        .await
        .unwrap()
        .unwrap()
        .id;
    session::Entity::insert(session::ActiveModel {
        token_hash: ActiveValue::Set("expired".into()),
        user_id: ActiveValue::Set(uid),
        created_at: ActiveValue::Set(1),
        expires_at: ActiveValue::Set(2),
        ..Default::default()
    })
    .exec(&state.db)
    .await
    .unwrap();

    for _ in 0..60 {
        chilled_api::snapshot::run_once(&state).await.unwrap();
    }
    let runs = snapshot_run::Entity::find().count(&state.db).await.unwrap();
    assert!(runs <= 50, "runs kept: {runs}");
    let expired = session::Entity::find().count(&state.db).await.unwrap();
    assert_eq!(expired, 0, "expired session survived the sweep");
}

#[tokio::test]
async fn artifacts_pagination_search_and_filters() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_with_admin(&dir);
    cfg.public_readonly = true;
    let scanner: Scanner = Arc::new(|| CacheStats {
        artifacts: (0..25)
            .map(|i| artifact(&format!("pkg-{i:02}"), "1.0.0", i as u64))
            .collect(),
        incomplete: false,
    });
    let (app, state) = router_with_scanners(cfg, vec![("npm".to_string(), scanner)]).await;
    chilled_api::snapshot::run_once(&state).await.unwrap();

    // Pagination.
    let page = body_json(send(&app, get("/api/artifacts?per_page=10&page=3")).await).await;
    assert_eq!(page["total"], 25);
    assert_eq!(page["items"].as_array().unwrap().len(), 5);
    assert_eq!(page["items"][0]["name"], "pkg-20");

    // Search narrows.
    let page = body_json(send(&app, get("/api/artifacts?q=pkg-1")).await).await;
    assert_eq!(page["total"], 10);

    // Sort by size descending.
    let page = body_json(send(&app, get("/api/artifacts?sort=size&order=desc")).await).await;
    assert_eq!(page["items"][0]["name"], "pkg-24");

    // Mount filter: a non-existent mount yields nothing.
    let page = body_json(send(&app, get("/api/artifacts?mount=maven")).await).await;
    assert_eq!(page["total"], 0);
    let page = body_json(send(&app, get("/api/artifacts?mount=npm&kind=npm")).await).await;
    assert_eq!(page["total"], 25);
}
