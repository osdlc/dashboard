//! Runs each plugin package in `examples/plugins` through the real plugin host and
//! checks what it sends, so the examples keep matching the contract. An
//! example whose interpreter isn't installed is skipped. None of these touch
//! the network except `gh_actions`, which only gets as far as `initialize`.

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::View;
use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{History, Module, Plugin};
use serde_json::json;

fn example(file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../../examples/plugins", file]
        .iter()
        .collect();
    path.to_string_lossy().into_owned()
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

fn problems(found: Vec<dashboard_contract::check::Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

/// initialize + `renders` renders, with host history applied like the app does.
async fn run(argv: &[&str], config: serde_json::Value, renders: usize) -> (String, View) {
    let mut plugin = Plugin::new("test", argv.iter().map(|s| s.to_string()).collect());
    let manifest = plugin.init(config).await.expect("initialize");
    assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
    let mut history = History::default();
    let mut last = None;
    for i in 0..renders {
        if i > 0 {
            // Points are stamped in milliseconds and the host drops one that
            // isn't newer than the last, so keep renders at least 1 ms apart.
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        let mut view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new());
        history.apply("test", &mut view);
        last = Some(view);
    }
    (manifest.id, last.unwrap())
}

#[tokio::test]
async fn hello() {
    if !have("python3") {
        return;
    }
    let (id, view) = run(
        &["python3", &example("hello/hello.py")],
        json!({ "name": "test" }),
        2,
    )
    .await;
    assert_eq!(id, "hello");
    assert_eq!(view.summary, "hello, test");
}

#[tokio::test]
async fn git_repos_finds_this_repository() {
    if !have("python3") || !have("git") {
        return;
    }
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let (_, view) = run(
        &["python3", &example("git-repos/git_repos.py")],
        json!({ "roots": [], "repos": [root] }),
        1,
    )
    .await;
    assert!(view.summary.starts_with("1 repos"), "{}", view.summary);
}

#[tokio::test]
async fn git_repos_rejects_unknown_settings() {
    if !have("python3") {
        return;
    }
    let mut plugin = Plugin::new(
        "test",
        vec!["python3".into(), example("git-repos/git_repos.py")],
    );
    let err = plugin.init(json!({ "rooots": ["~"] })).await.unwrap_err();
    assert!(
        format!("{err:#}").contains("unknown settings: rooots"),
        "{err:#}"
    );
}

#[tokio::test]
async fn latency_keeps_history_in_the_host() {
    if !have("python3") {
        return;
    }
    // A local listener, so the test needs no network.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || for _ in listener.incoming() {});
    let config = json!({ "targets": [
        { "name": "local", "host": "127.0.0.1", "port": port },
        { "name": "closed", "host": "127.0.0.1", "port": 9 },
    ] });
    let (_, view) = run(&["python3", &example("latency/latency.py")], config, 3).await;
    assert_eq!(view.status, dashboard_contract::Status::Error);
    let dashboard_contract::Widget::Column { children } = &view.root else {
        panic!("column")
    };
    let dashboard_contract::Widget::Series { series, .. } = &children[1] else {
        panic!("series")
    };
    assert_eq!(series[0].points.len(), 3, "host kept all three points");
    assert!(series[1].points.is_empty(), "a closed port sends no points");
}

#[tokio::test]
async fn gh_actions_needs_repos() {
    if !have("node") {
        return;
    }
    let argv = vec!["node".into(), example("gh-actions/gh_actions.mjs")];
    let err = Plugin::new("test", argv.clone())
        .init(json!({}))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("code -32002"), "{err:#}");
    let manifest = Plugin::new("test", argv)
        .init(json!({ "repos": ["osdlc/dashboard"] }))
        .await
        .unwrap();
    assert_eq!(manifest.id, "gh-actions");
}
