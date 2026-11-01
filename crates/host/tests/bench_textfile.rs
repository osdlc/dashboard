//! Runs the TypeScript and Python textfile plugins (`plugins/ext/textfile-ts`,
//! `plugins/ext/textfile-py`) through the real plugin host on the cases in
//! `crates/modules-system/testdata/textfile/cases.json`, and checks each view
//! against the expected one. The native module's tests check the same cases,
//! so all three show the same view for the same file. A plugin whose
//! interpreter isn't installed is skipped.

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::View;
use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn repo(path: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect()
}

fn data(name: &str) -> PathBuf {
    repo("crates/modules-system/testdata/textfile").join(name)
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

/// (id, argv) of each plugin whose interpreter is installed.
fn plugins() -> Vec<(&'static str, Vec<String>)> {
    let path = |p: &str| repo(p).to_string_lossy().into_owned();
    let mut out = vec![];
    if have("node") {
        out.push((
            "textfile-ts",
            vec!["node".into(), path("plugins/ext/textfile-ts/main.ts")],
        ));
    }
    if have("python3") {
        out.push((
            "textfile-py",
            vec!["python3".into(), path("plugins/ext/textfile-py/main.py")],
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

/// A folder of its own under the system temp folder, removed when dropped
/// (also when a test fails).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "dashboard-bench-textfile-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn plugins_match_the_expected_views() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(data("cases.json")).unwrap()).unwrap();
    for (id, argv) in plugins() {
        for case in &cases {
            let mut config = case["config"].clone();
            config["path"] = data(case["file"].as_str().unwrap())
                .to_string_lossy()
                .into();
            let mut plugin = Plugin::new("test", argv.clone());
            let manifest = plugin.init(config).await.expect("initialize");
            assert_eq!(manifest.id, id);
            assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
            let view = plugin.render().await.expect("view/render");
            assert_eq!(problems(check_view(&view)), Vec::<String>::new());
            let text = std::fs::read_to_string(data(case["view"].as_str().unwrap())).unwrap();
            let expected: View = serde_json::from_str(&text).unwrap();
            assert_eq!(view, expected, "{id} on {case}");
        }
    }
}

#[tokio::test]
async fn plugins_show_the_file_again_when_it_changes() {
    for (id, argv) in plugins() {
        let dir = TempDir::new(id);
        let path = dir.0.join("todo.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut plugin = Plugin::new("test", argv);
        plugin
            .init(json!({ "path": path }))
            .await
            .expect("initialize");
        let view = plugin.render().await.expect("view/render");
        assert_eq!(view.summary, "todo.txt · 1 line", "{id}");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let view = plugin.render().await.expect("view/render");
        assert_eq!(view.summary, "todo.txt · 2 lines", "{id}");

        std::fs::remove_file(&path).unwrap();
        let err = format!("{:#}", plugin.render().await.unwrap_err());
        assert!(err.contains("code -32001"), "{id}: {err}");
    }
}

#[tokio::test]
async fn plugins_reject_bad_settings() {
    for (id, argv) in plugins() {
        for (config, word) in [
            (json!({ "path": "x", "pth": "y" }), "pth"),
            (json!({}), "path"),
            (json!({ "path": "x", "lines": 0 }), "lines"),
            (json!({ "path": "x", "markdown": "yes" }), "markdown"),
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

#[test]
fn sdk_copies_match_the_sdk() {
    // Each package carries its own copy of the SDK, so it runs wherever it is
    // installed. Keep them in step with sdk/.
    for (copy, sdk) in [
        (
            "plugins/ext/textfile-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/textfile-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(repo(copy)).unwrap();
        let b = std::fs::read_to_string(repo(sdk)).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
