//! The ops plugins in `plugins/ext` (datadog, newrelic, opsgenie, pagerduty,
//! sentry), run as packages through the real plugin host. None of them has
//! been called live: each test points the plugin at a small local HTTP server
//! that answers with the package's hand-written sample response
//! (`sample/*.json`), and checks the request it sent and the view it built.
//! Skipped when `python3` isn't installed.

#![cfg(unix)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use dashboard_config::packages::{Installed, Packages};
use dashboard_contract::check::check_view;
use dashboard_contract::{Status, View, Widget};
use dashboard_host::{Module, Plugin};
use serde_json::{Value, json};

const IDS: [&str; 5] = ["datadog", "newrelic", "opsgenie", "pagerduty", "sentry"];

fn ext() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext")
}

fn have_python() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

fn package(id: &str) -> Installed {
    let found = Packages::load_from(&[ext()]);
    found
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("{id}: {:?}", found.errors))
}

/// A local HTTP server. Each path (without the query) answers with a status
/// and a file from the package's `sample/` folder. Every request's head is kept.
struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(id: &str, routes: &[(&str, u16, &str)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let routes: HashMap<String, (u16, String)> = routes
            .iter()
            .map(|(path, code, file)| {
                let body = if file.is_empty() {
                    "{}".to_string()
                } else {
                    std::fs::read_to_string(ext().join(id).join("sample").join(file)).unwrap()
                };
                (path.to_string(), (*code, body))
            })
            .collect();
        let requests = Arc::new(Mutex::new(vec![]));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = vec![];
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let target = head.split(' ').nth(1).unwrap_or("");
                let path = target.split('?').next().unwrap_or("");
                let (code, body) = routes
                    .get(path)
                    .cloned()
                    .unwrap_or((404, r#"{"error":"not found"}"#.into()));
                seen.lock().unwrap().push(head);
                let reply = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Self { url, requests }
    }

    /// The heads of the requests so far, lowercased (header names vary in case).
    fn requests(&self) -> Vec<String> {
        let seen = self.requests.lock().unwrap();
        seen.iter().map(|r| r.to_lowercase()).collect()
    }
}

fn plugin(id: &str) -> Plugin {
    let p = package(id);
    Plugin::new("test", p.argv()).in_dir(&p.dir)
}

/// Runs the package with `config` and returns its view, checked for problems.
async fn render(id: &str, config: Value) -> View {
    let mut plugin = plugin(id);
    plugin.init(config).await.expect("initialize");
    let view = plugin.render().await.expect("view/render");
    let problems: Vec<String> = check_view(&view)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    assert!(problems.is_empty(), "{id}: {problems:?}");
    view
}

/// The error a render fails with.
async fn render_error(id: &str, config: Value) -> String {
    let mut plugin = plugin(id);
    plugin.init(config).await.expect("initialize");
    format!("{:#}", plugin.render().await.unwrap_err())
}

/// The list's titles, the stats (label, value) and the list's links.
type Parts = (Vec<String>, Vec<(String, String)>, Vec<Option<String>>);

/// The list under the stats row, and the stats' values.
fn parts(view: &View) -> Parts {
    let Widget::Column { children } = &view.root else {
        panic!("column")
    };
    let Widget::Row { children: stats } = &children[0] else {
        panic!("stats row")
    };
    let stats = stats
        .iter()
        .map(|w| match w {
            Widget::Stat { label, value, .. } => (label.clone(), value.clone()),
            _ => panic!("stat"),
        })
        .collect();
    let Widget::List { items, .. } = &children[1] else {
        panic!("list")
    };
    let titles = items.iter().map(|i| i.title.clone()).collect();
    let urls = items
        .iter()
        .map(|i| match &i.action {
            Some(dashboard_contract::Action::OpenUrl { url }) => Some(url.clone()),
            _ => None,
        })
        .collect();
    (titles, stats, urls)
}

fn stat(stats: &[(String, String)], label: &str) -> String {
    stats
        .iter()
        .find(|(l, _)| l == label)
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| panic!("no stat {label}"))
}

/// The token every test hands the plugins, through a command.
fn token() -> Value {
    json!(["echo", "test-token"])
}

/// The demo view matches what the plugin builds from the same sample (status,
/// summary, stats, titles and links; not the ages, which depend on the clock),
/// so the palette preview shows what the tile will.
fn same_as_demo(id: &str, view: &View) {
    let demo = package(id).package.demo;
    assert_eq!(demo.status, view.status, "{id}: demo status");
    assert_eq!(demo.summary, view.summary, "{id}: demo summary");
    assert_eq!(
        parts(&demo),
        parts(view),
        "{id}: demo stats, titles or links"
    );
}

#[tokio::test]
async fn datadog_monitors() {
    if !have_python() {
        return;
    }
    let server = Server::start("datadog", &[("/api/v1/monitor", 200, "monitors.json")]);
    let view = render(
        "datadog",
        json!({ "base_url": server.url, "tags": ["team:web", "env:prod"],
                "token_command": token(), "app_key_command": ["echo", "test-app-key"] }),
    )
    .await;
    assert_eq!(view.status, Status::Error);
    same_as_demo("datadog", &view);
    let (titles, stats, urls) = parts(&view);
    assert_eq!(
        titles,
        [
            "API p95 latency above 800ms",
            "Checkout error rate",
            "Worker heartbeat"
        ]
    );
    assert_eq!(stat(&stats, "OK"), "3");
    assert_eq!(
        urls[0].as_deref(),
        Some("https://app.datadoghq.com/monitors/2481001")
    );
    let req = &server.requests()[0];
    assert!(
        req.starts_with("get /api/v1/monitor?monitor_tags=team%3aweb%2cenv%3aprod "),
        "{req}"
    );
    assert!(req.contains("dd-api-key: test-token"), "{req}");
    assert!(req.contains("dd-application-key: test-app-key"), "{req}");
}

#[tokio::test]
async fn datadog_show_ok_and_eu_links() {
    if !have_python() {
        return;
    }
    let server = Server::start("datadog", &[("/api/v1/monitor", 200, "monitors.json")]);
    let view = render(
        "datadog",
        json!({ "site": "datadoghq.eu", "base_url": server.url, "show_ok": true, "limit": 5,
                "token_command": token(), "app_key_command": token() }),
    )
    .await;
    let (titles, _, urls) = parts(&view);
    assert_eq!(titles.len(), 5);
    assert_eq!(
        urls[0].as_deref(),
        Some("https://app.datadoghq.eu/monitors/2481001")
    );
}

#[tokio::test]
async fn newrelic_violations() {
    if !have_python() {
        return;
    }
    let server = Server::start(
        "newrelic",
        &[("/v2/alerts_violations.json", 200, "alerts_violations.json")],
    );
    let view = render(
        "newrelic",
        json!({ "base_url": server.url, "account_id": 1234567, "token_command": token() }),
    )
    .await;
    assert_eq!(view.status, Status::Error);
    same_as_demo("newrelic", &view);
    let (titles, stats, urls) = parts(&view);
    assert_eq!(
        titles,
        [
            "High error rate: checkout-api",
            "Host CPU: db-2",
            "Low Apdex: web-frontend"
        ]
    );
    assert_eq!(stat(&stats, "Policies"), "3");
    assert_eq!(
        urls[0].as_deref(),
        Some("https://alerts.newrelic.com/accounts/1234567/incidents/55501")
    );
    let req = &server.requests()[0];
    assert!(
        req.starts_with("get /v2/alerts_violations.json?only_open=true "),
        "{req}"
    );
    assert!(req.contains("api-key: test-token"), "{req}");
}

#[tokio::test]
async fn newrelic_policy_filter() {
    if !have_python() {
        return;
    }
    let server = Server::start(
        "newrelic",
        &[("/v2/alerts_violations.json", 200, "alerts_violations.json")],
    );
    let view = render(
        "newrelic",
        json!({ "base_url": server.url, "policies": ["Web"], "token_command": token() }),
    )
    .await;
    assert_eq!(view.status, Status::Warn);
    assert_eq!(view.summary, "0 critical · 1 warning");
    let (_, _, urls) = parts(&view);
    assert_eq!(
        urls[0].as_deref(),
        Some("https://one.newrelic.com/alerts-ai")
    );
}

#[tokio::test]
async fn newrelic_eu_links() {
    if !have_python() {
        return;
    }
    let server = Server::start(
        "newrelic",
        &[("/v2/alerts_violations.json", 200, "alerts_violations.json")],
    );
    let view = render(
        "newrelic",
        json!({ "region": "eu", "base_url": server.url, "account_id": 1234567,
                "token_command": token() }),
    )
    .await;
    let (_, _, urls) = parts(&view);
    assert_eq!(
        urls[0].as_deref(),
        Some("https://alerts.eu.newrelic.com/accounts/1234567/incidents/55501")
    );
}

#[tokio::test]
async fn pagerduty_incidents_and_oncall() {
    if !have_python() {
        return;
    }
    let server = Server::start(
        "pagerduty",
        &[
            ("/incidents", 200, "incidents.json"),
            ("/oncalls", 200, "oncalls.json"),
        ],
    );
    let view = render(
        "pagerduty",
        json!({ "base_url": server.url, "team_ids": ["PTEAM01"], "token_command": token() }),
    )
    .await;
    assert_eq!(view.status, Status::Error);
    same_as_demo("pagerduty", &view);
    let (titles, stats, urls) = parts(&view);
    assert_eq!(titles[0], "#1842 Payments API returning 502s");
    assert_eq!(titles.len(), 3);
    assert_eq!(stat(&stats, "On call"), "Ana Ruiz, Sam Lee");
    assert_eq!(
        urls[0].as_deref(),
        Some("https://acme.pagerduty.com/incidents/Q1A2B3C4D5E6F7")
    );
    let reqs = server.requests();
    assert!(reqs[0].starts_with("get /incidents?"), "{}", reqs[0]);
    assert!(
        reqs[0].contains("statuses%5b%5d=triggered&statuses%5b%5d=acknowledged"),
        "{}",
        reqs[0]
    );
    assert!(reqs[0].contains("team_ids%5b%5d=pteam01"), "{}", reqs[0]);
    assert!(
        reqs[0].contains("authorization: token token=test-token"),
        "{}",
        reqs[0]
    );
    assert!(
        reqs[0].contains("accept: application/vnd.pagerduty+json;version=2"),
        "{}",
        reqs[0]
    );
    assert!(
        reqs[1].starts_with("get /oncalls?earliest=true"),
        "{}",
        reqs[1]
    );
}

#[tokio::test]
async fn pagerduty_without_oncall() {
    if !have_python() {
        return;
    }
    let server = Server::start("pagerduty", &[("/incidents", 200, "incidents.json")]);
    let view = render(
        "pagerduty",
        json!({ "base_url": server.url, "oncall": false, "limit": 1, "token_command": token() }),
    )
    .await;
    assert_eq!(view.summary, "1 triggered · 2 acknowledged");
    let (titles, stats, _) = parts(&view);
    assert_eq!(titles.len(), 1);
    assert_eq!(stats.len(), 2);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn pagerduty_checks_oncall_and_urgencies() {
    if !have_python() {
        return;
    }
    for (config, word) in [
        (json!({ "oncall": "no" }), "oncall"),
        (json!({ "urgencies": ["urgent"] }), "urgencies"),
    ] {
        let err = plugin("pagerduty").init(config).await.unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("code -32002") && err.contains(word), "{err}");
    }
}

#[tokio::test]
async fn opsgenie_alerts() {
    if !have_python() {
        return;
    }
    let server = Server::start("opsgenie", &[("/v2/alerts", 200, "alerts.json")]);
    let view = render(
        "opsgenie",
        json!({ "base_url": server.url, "account": "acme", "token_command": token() }),
    )
    .await;
    assert_eq!(view.status, Status::Error);
    same_as_demo("opsgenie", &view);
    let (titles, _, urls) = parts(&view);
    assert_eq!(
        titles,
        [
            "Payments API 5xx rate above 2%",
            "TLS certificate for www expires in 12 days",
            "Disk 91% full on db-2"
        ]
    );
    assert_eq!(
        urls[0].as_deref(),
        Some(
            "https://acme.app.opsgenie.com/alert/detail/70413a06-38d6-4c85-92b8-5ebc900d42e2/details"
        )
    );
    let req = &server.requests()[0];
    assert!(
        req.starts_with("get /v2/alerts?query=status%3aopen&limit=100"),
        "{req}"
    );
    assert!(req.contains("authorization: geniekey test-token"), "{req}");
}

#[tokio::test]
async fn opsgenie_eu_links() {
    if !have_python() {
        return;
    }
    let server = Server::start("opsgenie", &[("/v2/alerts", 200, "alerts.json")]);
    let view = render(
        "opsgenie",
        json!({ "region": "eu", "base_url": server.url, "account": "acme",
                "token_command": token() }),
    )
    .await;
    let (_, _, urls) = parts(&view);
    assert_eq!(
        urls[0].as_deref(),
        Some(
            "https://acme.app.eu.opsgenie.com/alert/detail/70413a06-38d6-4c85-92b8-5ebc900d42e2/details"
        )
    );
}

#[tokio::test]
async fn opsgenie_without_account_has_no_links() {
    if !have_python() {
        return;
    }
    let server = Server::start("opsgenie", &[("/v2/alerts", 200, "alerts.json")]);
    let view = render(
        "opsgenie",
        json!({ "base_url": server.url, "token_command": token() }),
    )
    .await;
    let (_, _, urls) = parts(&view);
    assert!(urls.iter().all(Option::is_none));
}

#[tokio::test]
async fn sentry_issues() {
    if !have_python() {
        return;
    }
    let server = Server::start(
        "sentry",
        &[("/api/0/organizations/acme/issues/", 200, "issues.json")],
    );
    let view = render(
        "sentry",
        json!({ "base_url": server.url, "org": "acme", "token_command": token() }),
    )
    .await;
    assert_eq!(view.status, Status::Error);
    same_as_demo("sentry", &view);
    let (titles, stats, urls) = parts(&view);
    assert_eq!(titles.len(), 3);
    assert_eq!(stat(&stats, "Users"), "331");
    assert_eq!(
        urls[0].as_deref(),
        Some("https://acme.sentry.io/issues/4521987001/")
    );
    let req = &server.requests()[0];
    assert!(
        req.starts_with("get /api/0/organizations/acme/issues/?query=is%3aunresolved"),
        "{req}"
    );
    assert!(req.contains("authorization: bearer test-token"), "{req}");
}

#[tokio::test]
async fn sentry_one_project() {
    if !have_python() {
        return;
    }
    let server = Server::start(
        "sentry",
        &[("/api/0/projects/acme/web/issues/", 200, "issues.json")],
    );
    render(
        "sentry",
        json!({ "base_url": server.url, "org": "acme", "project": "web",
                "environment": "production", "token_command": token() }),
    )
    .await;
    let req = &server.requests()[0];
    assert!(req.contains("environment=production"), "{req}");
}

/// What every plugin does when there is no token, the token is refused, or
/// the config has a typo.
#[tokio::test]
async fn errors() {
    if !have_python() {
        return;
    }
    for id in IDS {
        let server = Server::start(id, &[]);
        let base = json!({ "base_url": server.url, "org": "acme" });
        let with = |extra: Value| {
            let mut c = base.clone();
            if id != "sentry" {
                c.as_object_mut().unwrap().remove("org");
            }
            c.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            c
        };

        // No token: the variable it names is unset.
        let err = render_error(
            id,
            with(json!({ "token_env": "DASHBOARD_TEST_NO_SUCH_VAR" })),
        )
        .await;
        assert!(err.contains("code -32002"), "{id}: {err}");
        assert!(err.contains("$DASHBOARD_TEST_NO_SUCH_VAR"), "{id}: {err}");

        // A refused token (the server answers 404 to everything here).
        let mut extra = json!({ "token_command": token() });
        if id == "datadog" {
            extra["app_key_command"] = token();
        }
        let err = render_error(id, with(extra)).await;
        assert!(err.contains("code -32001"), "{id}: {err}");
        assert!(err.contains("HTTP 404"), "{id}: {err}");
        assert!(
            !err.contains("test-token"),
            "{id}: the error shows the token"
        );

        // A token command that reads stdin gets nothing, not the host's
        // requests, and fails at once instead of waiting.
        let started = std::time::Instant::now();
        let mut extra = json!({ "token_command": ["cat"] });
        if id == "datadog" {
            extra["app_key_command"] = json!(["cat"]);
        }
        let err = render_error(id, with(extra)).await;
        assert!(err.contains("printed nothing"), "{id}: {err}");
        assert!(started.elapsed().as_secs() < 5, "{id}: took too long");

        // token_env that is not a variable name.
        let err = Plugin::new("test", package(id).argv())
            .in_dir(package(id).dir)
            .init(with(json!({ "token_env": 5 })))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("code -32002") && err.contains("token_env"),
            "{id}: {err}"
        );

        // A typo in the config.
        let err = Plugin::new("test", package(id).argv())
            .in_dir(package(id).dir)
            .init(with(json!({ "limt": 3 })))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("code -32002") && err.contains("limt"),
            "{id}: {err}"
        );
    }
}

#[tokio::test]
async fn refused_token_says_so() {
    if !have_python() {
        return;
    }
    let server = Server::start("sentry", &[("/api/0/organizations/acme/issues/", 401, "")]);
    let err = render_error(
        "sentry",
        json!({ "base_url": server.url, "org": "acme", "token_command": token() }),
    )
    .await;
    assert!(err.contains("check the token"), "{err}");
}

#[test]
fn packages_load_and_say_untested_live() {
    let found = Packages::load_from(&[ext()]);
    for id in IDS {
        let p = found
            .get(id)
            .unwrap_or_else(|| panic!("{id}: {:?}", found.errors));
        assert!(p.package.description.contains("untested live"), "{id}");
    }
}

/// Each package carries copies of the SDK and of `ops.py`; they must match.
#[test]
fn shared_copies_are_current() {
    let read = |p: PathBuf| std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
    let sdk = read(ext().join("../../sdk/python/dashboard_plugin.py"));
    let ops = read(ext().join("datadog/ops.py"));
    for id in IDS {
        let dir = ext().join(id);
        assert!(
            read(dir.join("dashboard_plugin.py")) == sdk,
            "{id}: copy the SDK again"
        );
        assert!(
            read(dir.join("ops.py")) == ops,
            "{id}: copy datadog/ops.py again"
        );
    }
}
