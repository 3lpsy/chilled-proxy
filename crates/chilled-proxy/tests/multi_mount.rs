//! Several mounts of one registry: each reaches its own upstream and keeps its
//! own cache, which is what makes a Gradle build (Central + plugin portal +
//! Google Maven) servable from a single process.

use clap::Parser;
use serde_json::Value;
use tempfile::TempDir;
use wiremock::matchers::{method, path as match_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The Maven path of a jar, relative to a repository root.
const JAR: &str = "com/example/thing/1.0/thing-1.0.jar";

/// The POM the age probe HEADs for `JAR`.
const POM: &str = "/com/example/thing/1.0/thing-1.0.pom";

/// A running app plus the temp cache directory it writes into.
struct TestApp {
    base_url: String,
    client: reqwest::Client,
    tmp: TempDir,
}

impl TestApp {
    /// Starts the app with every registry but Maven disabled, plus `extra`.
    ///
    /// The built-in Gradle mounts are off here: they point at the real plugin
    /// portal and Google Maven, so these tests declare the mounts they mean to
    /// exercise. That the built-ins are served by default is covered by the CLI
    /// unit tests and by `endpoints.rs`.
    async fn start(extra: &[String]) -> TestApp {
        let tmp = TempDir::new().unwrap();
        let mut argv = vec![
            "chilled-proxy".to_string(),
            "--cache-dir".into(),
            tmp.path().to_string_lossy().into_owned(),
            "--no-default-mounts".into(),
            "--disable-crates".into(),
            "--disable-npm".into(),
            "--disable-pypi".into(),
        ];
        argv.extend(extra.iter().cloned());

        let cli = chilled_proxy::cli::Cli::try_parse_from(argv).unwrap();
        cli.check_mounts().expect("mounts are valid");
        let app = chilled_proxy::build_app(&cli);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(chilled_core::serve::serve_listener(listener, app));

        let client = reqwest::Client::new();
        let base_url = format!("http://{addr}");
        for _ in 0..100 {
            if client
                .get(format!("{base_url}/healthz"))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        TestApp {
            base_url,
            client,
            tmp,
        }
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .expect("request")
    }
}

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

    let app = TestApp::start(&[
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

    let app = TestApp::start(&[
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

    let app = TestApp::start(&[
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

    let app = TestApp::start(&[
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

    let app = TestApp::start(&[
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
