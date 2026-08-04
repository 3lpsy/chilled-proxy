//! Upstream credentials and custom headers actually reach the upstream, and
//! stay confined to the mount they were configured for.
//!
//! The env-var side (`CHILLED_<NAME>_BASIC_AUTH_*`) is covered by the `auth`
//! unit tests, which inject a lookup instead of mutating the process
//! environment — which is global, and these tests run in parallel.

use clap::Parser;
use tempfile::TempDir;
use wiremock::matchers::{method, path as match_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The Maven path of a jar, relative to a repository root.
const JAR: &str = "com/example/thing/1.0/thing-1.0.jar";

/// base64("alice:s3cr3t").
const EXPECTED_BASIC: &str = "Basic YWxpY2U6czNjcjN0";

struct TestApp {
    base_url: String,
    client: reqwest::Client,
    _tmp: TempDir,
}

impl TestApp {
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
            _tmp: tmp,
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

/// A repository that serves the jar to anyone, recording what it was sent.
async fn open_repo() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path(format!("/{JAR}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"jar".to_vec()))
        .mount(&server)
        .await;
    server
}

/// The value of `header` on the one request `server` received.
async fn received_header(server: &MockServer, header: &str) -> Option<String> {
    let requests = server.received_requests().await.expect("recording enabled");
    let request = requests.first().expect("upstream was called");
    request
        .headers
        .get(header)
        .map(|v| v.to_str().unwrap().to_owned())
}

#[tokio::test]
async fn basic_auth_reaches_the_upstream() {
    // The repository serves only when the credentials are present, so a 200
    // proves they were sent on the artifact fetch.
    let repo = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path(format!("/{JAR}")))
        .and(wiremock::matchers::header("authorization", EXPECTED_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"jar".to_vec()))
        .mount(&repo)
        .await;

    let app = TestApp::start(&[
        "--maven-upstream-url".into(),
        format!("{}/", repo.uri()),
        "--upstream-basic-auth".into(),
        "maven=alice:s3cr3t".into(),
    ])
    .await;

    let resp = app.get(&format!("/maven/{JAR}")).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "jar");
}

#[tokio::test]
async fn custom_headers_reach_the_upstream() {
    let repo = open_repo().await;
    let app = TestApp::start(&[
        "--maven-upstream-url".into(),
        format!("{}/", repo.uri()),
        "--upstream-header".into(),
        "maven=X-Build: ci".into(),
        "--upstream-header".into(),
        "maven=X-Api-Key: k123".into(),
    ])
    .await;

    assert_eq!(app.get(&format!("/maven/{JAR}")).await.status(), 200);
    assert_eq!(
        received_header(&repo, "x-build").await.as_deref(),
        Some("ci")
    );
    assert_eq!(
        received_header(&repo, "x-api-key").await.as_deref(),
        Some("k123")
    );
}

#[tokio::test]
async fn credentials_do_not_cross_mounts() {
    // Each authenticated mount gets its own client, so an unauthenticated mount
    // must send nothing — even to the same upstream host.
    let secured = open_repo().await;
    let open = open_repo().await;

    let app = TestApp::start(&[
        "--maven-upstream-url".into(),
        format!("{}/", secured.uri()),
        "--maven-mount".into(),
        format!("name=public,upstream={}/", open.uri()),
        "--upstream-basic-auth".into(),
        "maven=alice:s3cr3t".into(),
    ])
    .await;

    assert_eq!(app.get(&format!("/maven/{JAR}")).await.status(), 200);
    assert_eq!(app.get(&format!("/public/{JAR}")).await.status(), 200);

    assert_eq!(
        received_header(&secured, "authorization").await.as_deref(),
        Some(EXPECTED_BASIC)
    );
    assert_eq!(received_header(&open, "authorization").await, None);
}

#[tokio::test]
async fn auth_applies_to_both_of_a_mounts_urls() {
    // crates.io resolves through a sparse index and a separate download host.
    // Both are this mount's upstreams, so both carry its credentials.
    // `config.json` is generated by the proxy, so drive a real sparse-index
    // lookup instead: `serde` lives at `/se/rd/serde`.
    let index = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path("/se/rd/serde"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "{\"name\":\"serde\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"\",\"features\":{},\"yanked\":false}\n",
        ))
        .mount(&index)
        .await;

    let downloads = MockServer::start().await;
    Mock::given(method("GET"))
        .and(match_path("/api/v1/crates/serde/1.0.0/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"crate-bytes".to_vec()))
        .mount(&downloads)
        .await;

    let tmp = TempDir::new().unwrap();
    let cli = chilled_proxy::cli::Cli::try_parse_from([
        "chilled-proxy".to_string(),
        "--cache-dir".into(),
        tmp.path().to_string_lossy().into_owned(),
        "--no-default-mounts".into(),
        "--disable-npm".into(),
        "--disable-pypi".into(),
        "--disable-maven".into(),
        "--crates-index-url".into(),
        format!("{}/", index.uri()),
        "--crates-upstream-url".into(),
        format!("{}/", downloads.uri()),
        "--upstream-basic-auth".into(),
        "crates=alice:s3cr3t".into(),
    ])
    .unwrap();
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

    // The index URL...
    assert_eq!(
        client
            .get(format!("{base_url}/crates/index/se/rd/serde"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        received_header(&index, "authorization").await.as_deref(),
        Some(EXPECTED_BASIC)
    );

    // ...and the download URL.
    assert_eq!(
        client
            .get(format!(
                "{base_url}/crates/api/v1/crates/serde/1.0.0/download"
            ))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        received_header(&downloads, "authorization")
            .await
            .as_deref(),
        Some(EXPECTED_BASIC)
    );
}
