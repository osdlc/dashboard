//! The built-in hackernews module and its TypeScript and Python copies in
//! `plugins/ext` must show the same view for the same input, so the
//! native-vs-plugin benchmark compares like with like. Each reads the
//! hand-written sample in `testdata/hackernews` (no network). The plugins run
//! through the real plugin host. A plugin whose interpreter isn't installed
//! (or, for node, is older than 24) is skipped.

#![cfg(feature = "hackernews")]

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn repo(path: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect();
    path.to_string_lossy().into_owned()
}

fn sample() -> String {
    repo("crates/modules-fun/testdata/hackernews/top.json")
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

/// Node 24 or later, which runs `main.ts` without a build step.
fn have_node() -> bool {
    let Ok(out) = Command::new("node").arg("--version").output() else {
        return false;
    };
    let version = String::from_utf8_lossy(&out.stdout);
    let major = version.trim().trim_start_matches('v').split('.').next();
    major
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|m| m >= 24)
}

/// (id, argv) of each plugin whose interpreter is installed.
fn plugins() -> Vec<(&'static str, Vec<String>)> {
    let mut out = vec![];
    if have_node() {
        out.push((
            "hackernews-ts",
            vec!["node".into(), repo("plugins/ext/hackernews-ts/main.ts")],
        ));
    }
    if have("python3") {
        out.push((
            "hackernews-py",
            vec!["python3".into(), repo("plugins/ext/hackernews-py/main.py")],
        ));
    }
    out
}

fn problems(found: Vec<dashboard_contract::check::Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

/// Every implementation renders `config`; each plugin's view must equal the
/// native one.
async fn same(config: serde_json::Value) {
    let mut native = dashboard_modules_fun::hackernews::HackerNews::default();
    native.init(config.clone()).await.unwrap();
    let expected = native.render().await.unwrap();
    for (id, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv);
        let manifest = plugin.init(config.clone()).await.expect("initialize");
        assert_eq!(manifest.id, id);
        assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
        let view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new(), "{id}");
        assert_eq!(view, expected, "{id} differs from native");
    }
}

#[tokio::test]
async fn same_view_for_the_sample() {
    same(json!({ "sample": sample() })).await;
}

#[tokio::test]
async fn same_view_with_a_limit_and_another_feed() {
    same(json!({ "sample": sample(), "limit": 3, "feed": "ask" })).await;
}

#[tokio::test]
async fn same_view_with_no_stories() {
    same(json!({ "sample": sample(), "limit": 0 })).await;
}

#[tokio::test]
async fn plugins_reject_bad_settings() {
    for (id, argv) in plugins() {
        for (config, word) in [
            (json!({ "limt": 5 }), "limt"),
            (json!({ "feed": "hot" }), "feed"),
            (json!({ "limit": -1 }), "limit"),
            (json!({ "limit": 31 }), "limit"),
        ] {
            let err = Plugin::new("test", argv.clone())
                .init(config)
                .await
                .unwrap_err();
            let err = format!("{err:#}");
            assert!(err.contains("code -32002"), "{id}: {err}");
            assert!(err.contains(word), "{id}: {err}");
        }
    }
}

#[tokio::test]
async fn a_missing_or_bad_sample_is_an_upstream_error() {
    for (id, argv) in plugins() {
        for path in ["/no/such/file", &repo("crates/modules-fun/Cargo.toml")] {
            let mut plugin = Plugin::new("test", argv.clone());
            plugin
                .init(json!({ "sample": path }))
                .await
                .expect("initialize");
            let err = format!("{:#}", plugin.render().await.unwrap_err());
            assert!(err.contains("code -32001"), "{id}: {err}");
        }
    }
}

#[test]
fn sdk_copies_match_the_sdk() {
    // Each package in plugins/ext carries its own copy of the SDK, so it runs
    // wherever it is installed. Keep every copy in step with sdk/.
    let sdks = [
        ("dashboard-plugin.ts", "sdk/typescript/dashboard-plugin.ts"),
        ("dashboard_plugin.py", "sdk/python/dashboard_plugin.py"),
    ];
    let mut checked = 0;
    for dir in std::fs::read_dir(repo("plugins/ext")).unwrap() {
        let dir = dir.unwrap().path();
        for (name, sdk) in sdks {
            let copy = dir.join(name);
            if !copy.exists() {
                continue;
            }
            let a = std::fs::read_to_string(&copy).unwrap();
            let b = std::fs::read_to_string(repo(sdk)).unwrap();
            assert!(
                a == b,
                "{} differs from {sdk}: copy it again",
                copy.display()
            );
            checked += 1;
        }
    }
    assert!(checked >= 2, "found no SDK copies in plugins/ext");
}
