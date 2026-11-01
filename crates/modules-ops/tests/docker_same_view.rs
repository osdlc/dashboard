//! The built-in docker module and its TypeScript and Python copies in
//! `plugins/ext` must show the same view for the same docker output, so the
//! native-vs-plugin benchmark compares like with like. Each reads a fake
//! `docker` that prints recorded output. A plugin whose interpreter isn't
//! installed is skipped.

#![cfg(all(unix, feature = "docker"))]

use std::path::PathBuf;
use std::process::Command;

use dashboard_host::{Module, Plugin};
use serde_json::json;

fn repo(path: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect();
    path.to_string_lossy().into_owned()
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

/// The view each implementation renders for `config`, as JSON.
async fn views(config: serde_json::Value) -> Vec<(String, serde_json::Value)> {
    let mut native = dashboard_modules_ops::docker::Docker::default();
    native.init(config.clone()).await.unwrap();
    let mut out = vec![(
        "native".to_string(),
        serde_json::to_value(native.render().await.unwrap()).unwrap(),
    )];
    for (runner, file) in [
        ("node", "plugins/ext/docker-ts/main.ts"),
        ("python3", "plugins/ext/docker-py/main.py"),
    ] {
        if !have(runner) {
            continue;
        }
        let mut plugin = Plugin::new("test", vec![runner.into(), repo(file)]);
        plugin.init(config.clone()).await.expect("initialize");
        let view = plugin.render().await.expect("view/render");
        out.push((runner.to_string(), serde_json::to_value(view).unwrap()));
    }
    out
}

async fn same(config: serde_json::Value) {
    let all = views(config).await;
    let (_, native) = &all[0];
    for (name, view) in &all[1..] {
        assert_eq!(view, native, "{name} differs from native");
    }
}

fn fake(name: &str) -> String {
    repo(&format!("crates/modules-ops/testdata/docker/{name}"))
}

#[tokio::test]
async fn same_view_with_stats() {
    same(json!({ "docker": fake("docker") })).await;
}

#[tokio::test]
async fn same_view_without_stats_and_with_a_limit() {
    same(json!({ "docker": fake("docker"), "stats": false, "limit": 2 })).await;
}

#[tokio::test]
async fn same_view_when_the_daemon_is_down() {
    same(json!({ "docker": fake("docker-down") })).await;
}

#[tokio::test]
async fn same_view_when_docker_is_missing() {
    same(json!({ "docker": "/no/such/docker" })).await;
}

/// The packages write their demo view out by hand; it must match the built-in
/// module's, which is built from the recorded output.
#[test]
fn package_demos_match_the_builtin_demo() {
    let native = serde_json::to_value(dashboard_modules_ops::docker::catalog().demo).unwrap();
    for dir in ["plugins/ext/docker-ts", "plugins/ext/docker-py"] {
        let installed = dashboard_config::packages::load_package(std::path::Path::new(&repo(dir)))
            .unwrap_or_else(|e| panic!("{dir}: {e:#}"));
        let demo = serde_json::to_value(installed.package.demo).unwrap();
        assert_eq!(demo, native, "{dir} demo differs from the built-in demo");
    }
}
