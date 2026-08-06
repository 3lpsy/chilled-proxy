//! Several mounts of one registry: each reaches its own upstream and keeps its
//! own cache, which is what makes a Gradle build (Central + plugin portal +
//! Google Maven) servable from a single process.

// The built-in Gradle mounts are off in these tests (`start_maven_only`): they
// point at the real plugin portal and Google Maven, so each test declares the
// mounts it means to exercise. That the built-ins are served by default is
// covered by the CLI unit tests and by `endpoints.rs`.

mod common;

use common::TestApp;
use serde_json::Value;
use wiremock::matchers::{method, path as match_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The Maven path of a jar, relative to a repository root.
const JAR: &str = "com/example/thing/1.0/thing-1.0.jar";

/// The POM the age probe HEADs for `JAR`.
const POM: &str = "/com/example/thing/1.0/thing-1.0.pom";

/// A mock Maven repository serving `body` for the one jar under test.
async fn mock_repo(body: &'static str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path(format!("/{JAR}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.as_bytes().to_vec()))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn each_mount_fetches_from_its_own_upstream() {
    let central = mock_repo("from-central").await;
    let portal = mock_repo("from-portal").await;

    let app = TestApp::start_maven_only(&[
        "--maven-upstream-url".into(),
        format!("{}/", central.uri()),
        "--maven-mount".into(),
        format!(
            "name=plugins,path=/gradle-plugins,upstream={}/",
            portal.uri()
        ),
    ])
    .await;

    let default = app.get(&format!("/maven/{JAR}")).await;
    assert_eq!(default.status(), 200);
    assert_eq!(default.text().await.unwrap(), "from-central");

    let extra = app.get(&format!("/gradle-plugins/{JAR}")).await;
    assert_eq!(extra.status(), 200);
    assert_eq!(extra.text().await.unwrap(), "from-portal");
}

#[tokio::test]
async fn mounts_do_not_share_a_cache() {
    // Same coordinates from two upstreams must not collide on disk, or the
    // second mount would serve the first one's bytes.
    let central = mock_repo("from-central").await;
    let portal = mock_repo("from-portal").await;

    let app = TestApp::start_maven_only(&[
        "--maven-upstream-url".into(),
        format!("{}/", central.uri()),
        "--maven-mount".into(),
        format!("name=plugins,upstream={}/", portal.uri()),
    ])
    .await;

    app.get(&format!("/maven/{JAR}")).await;
    app.get(&format!("/plugins/{JAR}")).await;

    let cached =
        |name: &str| std::fs::read_to_string(app.tmp.path().join(name).join("repo").join(JAR));
    assert_eq!(cached("maven").unwrap(), "from-central");
    assert_eq!(cached("plugins").unwrap(), "from-portal");

    // Served again from cache, still from the right one.
    let second = app.get(&format!("/plugins/{JAR}")).await;
    assert_eq!(second.text().await.unwrap(), "from-portal");
}

#[tokio::test]
async fn every_mount_is_listed_and_reported() {
    let central = mock_repo("from-central").await;
    let portal = mock_repo("from-portal").await;

    let app = TestApp::start_maven_only(&[
        "--enable-metrics".into(),
        "--maven-upstream-url".into(),
        format!("{}/", central.uri()),
        "--maven-mount".into(),
        format!("name=plugins,upstream={}/", portal.uri()),
    ])
    .await;

    let home: Value = app.get("/").await.json().await.unwrap();
    let names: Vec<&str> = home["registries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(names, ["maven", "plugins"]);

    app.get(&format!("/plugins/{JAR}")).await;
    let metrics: Value = app.get("/metrics").await.json().await.unwrap();
    // Each mount reports under its own name, so the two never merge.
    assert_eq!(metrics["registries"]["maven"]["cached_count"], 0);
    assert_eq!(metrics["registries"]["plugins"]["cached_count"], 1);
}

#[tokio::test]
async fn a_disabled_registry_still_serves_its_extra_mounts() {
    let portal = mock_repo("from-portal").await;

    let app = TestApp::start_maven_only(&[
        "--disable-maven".into(),
        "--maven-mount".into(),
        format!("name=plugins,upstream={}/", portal.uri()),
    ])
    .await;

    assert_eq!(app.get(&format!("/maven/{JAR}")).await.status(), 404);
    let extra = app.get(&format!("/plugins/{JAR}")).await;
    assert_eq!(extra.status(), 200);
    assert_eq!(extra.text().await.unwrap(), "from-portal");
}

#[tokio::test]
async fn a_mount_can_carry_its_own_cooldown() {
    // The portal mount refuses downloads inside its window while the default
    // Maven mount, with no cooldown, keeps serving.
    let central = mock_repo("from-central").await;
    let portal = mock_repo("from-portal").await;
    // The portal carries the version and dates it far in the future, so it is
    // squarely inside the window. Without this the probe would 404 and the
    // version would be absent — a 404, not a refusal.
    Mock::given(method("HEAD"))
        .and(match_path(POM.to_owned()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Last-Modified", "Wed, 01 Jan 2999 00:00:00 GMT"),
        )
        .mount(&portal)
        .await;

    let app = TestApp::start_maven_only(&[
        "--maven-upstream-url".into(),
        format!("{}/", central.uri()),
        "--maven-mount".into(),
        format!(
            "name=plugins,upstream={}/,cooldown=52w,restrict-downloads=true",
            portal.uri()
        ),
    ])
    .await;

    assert_eq!(app.get(&format!("/maven/{JAR}")).await.status(), 200);
    // Fail-closed: the age probe cannot clear a version inside the window.
    assert_eq!(app.get(&format!("/plugins/{JAR}")).await.status(), 403);
}

/// A minimal full npm packument for one version, tarball hosted at `upstream`.
fn npm_packument(name: &str, upstream: &str) -> String {
    format!(
        r#"{{"name":"{name}","dist-tags":{{"latest":"1.0.0"}},"versions":{{"1.0.0":{{"name":"{name}","version":"1.0.0","dist":{{"tarball":"{upstream}{name}/-/{name}-1.0.0.tgz"}}}}}},"time":{{"1.0.0":"2000-01-01T00:00:00Z"}}}}"#
    )
}

#[tokio::test]
async fn npm_mounts_rewrite_tarballs_to_their_own_mount() {
    let reg_a = MockServer::start().await;
    let reg_b = MockServer::start().await;
    for reg in [&reg_a, &reg_b] {
        Mock::given(method("GET"))
            .and(match_path("/foo"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(npm_packument("foo", &format!("{}/", reg.uri()))),
            )
            .mount(reg)
            .await;
    }

    let app = TestApp::start_bare(&[
        "--no-default-mounts".into(),
        "--disable-crates".into(),
        "--disable-pypi".into(),
        "--disable-maven".into(),
        "--npm-upstream-url".into(),
        format!("{}/", reg_a.uri()),
        "--npm-mount".into(),
        format!("name=npm2,upstream={}/", reg_b.uri()),
    ])
    .await;

    let a: Value = app.get("/npm/foo").await.json().await.unwrap();
    let b: Value = app.get("/npm2/foo").await.json().await.unwrap();
    let tarball = |doc: &Value| {
        doc["versions"]["1.0.0"]["dist"]["tarball"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    // Each mount rewrites tarball URLs to its own external mount URL.
    assert!(tarball(&a).contains("/npm/foo/-/"), "got {}", tarball(&a));
    assert!(tarball(&b).contains("/npm2/foo/-/"), "got {}", tarball(&b));
}

/// A minimal PEP 691 simple index for one file hosted at `upstream`.
fn simple_index(name: &str, upstream: &str) -> String {
    format!(
        r#"{{"meta":{{"api-version":"1.0"}},"name":"{name}","versions":["1.0.0"],"files":[{{"filename":"{name}-1.0.0.tar.gz","url":"{upstream}packages/{name}-1.0.0.tar.gz","hashes":{{}},"upload-time":"2000-01-01T00:00:00Z"}}]}}"#
    )
}

#[tokio::test]
async fn pypi_mounts_rewrite_file_urls_to_their_own_mount() {
    const SIMPLE_CTYPE: &str = "application/vnd.pypi.simple.v1+json";
    let idx_a = MockServer::start().await;
    let idx_b = MockServer::start().await;
    for idx in [&idx_a, &idx_b] {
        Mock::given(method("GET"))
            .and(match_path("/foo/"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                simple_index("foo", &format!("{}/", idx.uri())),
                SIMPLE_CTYPE,
            ))
            .mount(idx)
            .await;
    }

    let app = TestApp::start_bare(&[
        "--no-default-mounts".into(),
        "--disable-crates".into(),
        "--disable-npm".into(),
        "--disable-maven".into(),
        "--pypi-upstream-url".into(),
        format!("{}/", idx_a.uri()),
        "--pypi-files-url".into(),
        format!("{}/", idx_a.uri()),
        "--pypi-mount".into(),
        format!("name=pypi2,upstream={0}/,files={0}/", idx_b.uri()),
    ])
    .await;

    let file_url = |body: &Value| body["files"][0]["url"].as_str().unwrap().to_owned();
    let get = |path: &'static str| {
        let client = app.client.clone();
        let url = format!("{}{}", app.base_url, path);
        async move {
            client
                .get(url)
                .header("accept", SIMPLE_CTYPE)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };

    // Each mount rewrites file URLs to its own external mount URL — even for
    // the same project name, so the per-mount memo caches cannot cross-serve.
    let a = get("/pypi/simple/foo/").await;
    let b = get("/pypi2/simple/foo/").await;
    assert!(
        file_url(&a).contains("/pypi/files/foo/"),
        "got {}",
        file_url(&a)
    );
    assert!(
        file_url(&b).contains("/pypi2/files/foo/"),
        "got {}",
        file_url(&b)
    );

    // Second round comes from each mount's own caches, still not crossed.
    let a = get("/pypi/simple/foo/").await;
    let b = get("/pypi2/simple/foo/").await;
    assert!(file_url(&a).contains("/pypi/files/foo/"));
    assert!(file_url(&b).contains("/pypi2/files/foo/"));
}

#[tokio::test]
async fn crates_mounts_generate_their_own_config_json() {
    let up_a = MockServer::start().await;
    let up_b = MockServer::start().await;

    let app = TestApp::start_bare(&[
        "--no-default-mounts".into(),
        "--disable-npm".into(),
        "--disable-pypi".into(),
        "--disable-maven".into(),
        "--crates-index-url".into(),
        format!("{}/", up_a.uri()),
        "--crates-upstream-url".into(),
        format!("{}/", up_a.uri()),
        "--crates-mount".into(),
        format!("name=crates2,index={0}/,upstream={0}/", up_b.uri()),
    ])
    .await;

    let a: Value = app
        .get("/crates/index/config.json")
        .await
        .json()
        .await
        .unwrap();
    let b: Value = app
        .get("/crates2/index/config.json")
        .await
        .json()
        .await
        .unwrap();
    // Each mount's dl URL points back at that mount, and api at its upstream.
    assert!(a["dl"].as_str().unwrap().ends_with("/crates/api/v1/crates"));
    assert!(b["dl"]
        .as_str()
        .unwrap()
        .ends_with("/crates2/api/v1/crates"));
    assert!(a["api"].as_str().unwrap().starts_with(&up_a.uri()));
    assert!(b["api"].as_str().unwrap().starts_with(&up_b.uri()));
}
