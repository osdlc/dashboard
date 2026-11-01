//! Runs the TypeScript and Python docker plugins in `plugins/ext` (the
//! benchmark copies of the built-in `docker` module) through the real plugin
//! host. They read a fake `docker` that prints recorded output, so no Docker
//! is needed. A plugin whose interpreter isn't installed is skipped. The test
//! that all three build the same view is in `crates/modules-ops/tests`.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::check::check_view;
use dashboard_contract::{Cell, Status, View, Widget};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn repo(path: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect();
    path.to_string_lossy().into_owned()
}

/// The fake docker programs, next to the recorded output they print.
fn fake(name: &str) -> String {
    repo(&format!("crates/modules-ops/testdata/docker/{name}"))
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

/// Both plugins whose interpreter is installed.
fn plugins() -> Vec<Vec<String>> {
    let mut out = vec![];
    if have("node") {
        out.push(vec!["node".into(), repo("plugins/ext/docker-ts/main.ts")]);
    }
    if have("python3") {
        out.push(vec![
            "python3".into(),
            repo("plugins/ext/docker-py/main.py"),
        ]);
    }
    out
}

async fn render(argv: &[String], config: serde_json::Value) -> View {
    let mut plugin = Plugin::new("test", argv.to_vec());
    plugin.init(config).await.expect("initialize");
    let view = plugin.render().await.expect("view/render");
    let problems: Vec<String> = check_view(&view)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    assert!(problems.is_empty(), "{argv:?}: {problems:?}");
    view
}

#[tokio::test]
async fn shows_containers_with_cpu_and_memory() {
    for argv in plugins() {
        let view = render(&argv, json!({ "docker": fake("docker") })).await;
        assert_eq!(view.status, Status::Warn, "{argv:?}");
        assert_eq!(
            view.summary, "4 containers · 1 unhealthy · 1 restarting",
            "{argv:?}"
        );
        let Widget::Table { columns, rows } = &view.root else {
            panic!("{argv:?}: table")
        };
        assert_eq!(columns.len(), 5, "{argv:?}");
        assert_eq!(rows.len(), 4, "{argv:?}");
        assert_eq!(
            rows[2].cells["mem"],
            Cell::Text("512.4MiB".into()),
            "{argv:?}"
        );
    }
}

#[tokio::test]
async fn stats_off_and_limit() {
    for argv in plugins() {
        let view = render(
            &argv,
            json!({ "docker": fake("docker"), "stats": false, "limit": 1 }),
        )
        .await;
        let Widget::Table { columns, rows } = &view.root else {
            panic!("{argv:?}: table")
        };
        assert_eq!(columns.len(), 3, "{argv:?}");
        assert_eq!(rows.len(), 1, "{argv:?}");
    }
}

#[tokio::test]
async fn daemon_down_is_an_error_tile() {
    for argv in plugins() {
        let view = render(&argv, json!({ "docker": fake("docker-down") })).await;
        assert_eq!(view.status, Status::Error, "{argv:?}");
        assert_eq!(view.summary, "Docker is not running", "{argv:?}");
    }
}

#[tokio::test]
async fn missing_docker_is_an_error_tile() {
    for argv in plugins() {
        let view = render(&argv, json!({ "docker": "/no/such/docker" })).await;
        assert_eq!(view.status, Status::Error, "{argv:?}");
        assert_eq!(view.summary, "docker not found", "{argv:?}");
    }
}

#[tokio::test]
async fn rejects_unknown_settings() {
    for argv in plugins() {
        let err = Plugin::new("test", argv.clone())
            .init(json!({ "limt": 3 }))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("code -32002"), "{argv:?}: {err}");
        assert!(err.contains("limt"), "{argv:?}: {err}");
    }
}

/// Each package carries a copy of its SDK; it must match the one in `sdk/`.
#[test]
fn sdk_copies_are_current() {
    for (copy, sdk) in [
        (
            "plugins/ext/docker-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/docker-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(repo(copy)).unwrap();
        let b = std::fs::read_to_string(repo(sdk)).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
