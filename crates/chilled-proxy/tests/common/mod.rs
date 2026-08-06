//! Shared full-app harness: argv → resolved config → router → real HTTP,
//! built on `chilled_testkit::serve_app`.

#![allow(dead_code)]

use clap::Parser;
use tempfile::TempDir;
use wiremock::MockServer;

/// A running full app + optional shared mock upstream + temp cache dir.
pub struct TestApp {
    pub base_url: String,
    /// The one mock every registry points at (only for [`TestApp::start`]).
    pub mock_upstream: Option<MockServer>,
    pub client: reqwest::Client,
    pub tmp: TempDir,
}

impl TestApp {
    /// Starts the full app with every registry pointed at one fresh mock
    /// upstream, plus `extra` CLI args.
    pub async fn start(extra: &[&str]) -> TestApp {
        let mock_upstream = MockServer::start().await;
        let upstream = format!("{}/", mock_upstream.uri());
        let args: Vec<String> = [
            "--crates-index-url",
            &upstream,
            "--crates-upstream-url",
            &upstream,
            "--npm-upstream-url",
            &upstream,
            "--pypi-upstream-url",
            &upstream,
            "--pypi-files-url",
            &upstream,
            "--maven-upstream-url",
            &upstream,
        ]
        .iter()
        .map(ToString::to_string)
        .chain(extra.iter().map(ToString::to_string))
        .collect();

        let mut app = TestApp::start_bare(&args).await;
        app.mock_upstream = Some(mock_upstream);
        app
    }

    /// Starts the app with every registry but Maven disabled and no built-in
    /// mounts, plus `extra`. Mounts under test declare their own upstreams.
    pub async fn start_maven_only(extra: &[String]) -> TestApp {
        let mut args: Vec<String> = [
            "--no-default-mounts",
            "--disable-crates",
            "--disable-npm",
            "--disable-pypi",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        args.extend(extra.iter().cloned());
        TestApp::start_bare(&args).await
    }

    /// Starts the app with only `--cache-dir <tmp>` plus `args`.
    pub async fn start_bare(args: &[String]) -> TestApp {
        let tmp = TempDir::new().unwrap();
        let mut argv = vec![
            "chilled-proxy".to_string(),
            "--cache-dir".into(),
            tmp.path().to_string_lossy().into_owned(),
        ];
        argv.extend(args.iter().cloned());

        let cli = chilled_proxy::cli::Cli::try_parse_from(argv).unwrap();
        let config = cli.resolve().expect("configuration resolves");
        let app = chilled_proxy::build_app(&config);
        let (base_url, client) = chilled_testkit::serve_app(app, "/healthz").await;

        TestApp {
            base_url,
            mock_upstream: None,
            client,
            tmp,
        }
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .expect("request")
    }
}
