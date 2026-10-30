//! Runs the TypeScript and Python SDK examples in `sdk/*/examples` through the
//! real plugin host and checks what they send, so the SDKs keep matching the
//! contract. An example whose interpreter isn't installed is skipped. None of
//! these touch the network.

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::check::{check_manifest, check_view};
use dashboard_contract::{Status, View, Widget};
use dashboard_host::{History, Module, Plugin};
use serde_json::json;

fn sdk_file(path: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../../sdk", path]
        .iter()
        .collect();
    path.to_string_lossy().into_owned()
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

/// The interpreters for each SDK that are installed: Node and Bun for
/// TypeScript, python3 for Python.
fn runners(lang: &str) -> Vec<&'static str> {
    let all: &[&str] = if lang == "typescript" {
        &["node", "bun"]
    } else {
        &["python3"]
    };
    all.iter().copied().filter(|p| have(p)).collect()
}

fn problems(found: Vec<dashboard_contract::check::Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

/// initialize + `renders` renders, with host history applied like the app does.
async fn run(argv: Vec<String>, config: serde_json::Value, renders: usize) -> (String, View) {
    let mut plugin = Plugin::new("test", argv);
    let manifest = plugin.init(config).await.expect("initialize");
    assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
    let mut history = History::default();
    let mut last = None;
    for i in 0..renders {
        if i > 0 {
            // Points are stamped in ms; two renders in the same ms would share a stamp.
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let mut view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new());
        history.apply("test", &mut view);
        last = Some(view);
    }
    (manifest.id, last.unwrap())
}

/// Each (runner, example file) pair for both SDKs, e.g. ("node", ".../hello.ts").
fn each(name: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for runner in runners("typescript") {
        out.push(vec![
            runner.into(),
            sdk_file(&format!("typescript/examples/{name}.ts")),
        ]);
    }
    for runner in runners("python") {
        out.push(vec![
            runner.into(),
            sdk_file(&format!("python/examples/{name}.py")),
        ]);
    }
    out
}

#[tokio::test]
async fn hello() {
    for argv in each("hello") {
        let (id, view) = run(argv.clone(), json!({ "name": "test" }), 2).await;
        assert_eq!(id, "hello", "{argv:?}");
        assert_eq!(view.summary, "hello, test", "{argv:?}");
    }
}

#[tokio::test]
async fn hello_rejects_unknown_settings() {
    for argv in each("hello") {
        let err = Plugin::new("test", argv.clone())
            .init(json!({ "nmae": "x" }))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("code -32002"), "{argv:?}: {err}");
        assert!(err.contains("nmae"), "{argv:?}: {err}");
    }
}

#[tokio::test]
async fn big_files_shows_stats_and_a_table() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../sdk");
    for argv in each("big_files") {
        let (id, view) = run(argv.clone(), json!({ "dir": dir, "top": 3 }), 1).await;
        assert_eq!(id, "big-files", "{argv:?}");
        let Widget::Column { children } = &view.root else {
            panic!("{argv:?}: column")
        };
        assert!(
            matches!(children[0], Widget::Row { .. }),
            "{argv:?}: stats row"
        );
        let Some(Widget::Table { rows, .. }) = children.get(1) else {
            panic!("{argv:?}: table")
        };
        assert_eq!(rows.len(), 3, "{argv:?}");
    }
}

#[tokio::test]
async fn big_files_rejects_a_missing_folder() {
    for argv in each("big_files") {
        let err = Plugin::new("test", argv.clone())
            .init(json!({ "dir": "/no/such/folder" }))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("code -32002"), "{argv:?}: {err}");
        assert!(err.contains("not a folder"), "{argv:?}: {err}");
    }
}

#[tokio::test]
async fn load_keeps_history_in_the_host() {
    for argv in each("load") {
        let (id, view) = run(argv.clone(), json!({}), 3).await;
        assert_eq!(id, "load", "{argv:?}");
        assert_ne!(view.status, Status::Unknown, "{argv:?}");
        let Widget::Column { children } = &view.root else {
            panic!("{argv:?}: column")
        };
        let Widget::Series { series, .. } = &children[1] else {
            panic!("{argv:?}: series")
        };
        assert_eq!(series[0].points.len(), 3, "{argv:?}: host kept all three");
    }
}
