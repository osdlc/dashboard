//! Runs the TypeScript and Python clock plugins (`plugins/ext/clock-ts`,
//! `plugins/ext/clock-py`) through the real plugin host on the sample configs
//! in `crates/modules-fun/testdata/clock`, and checks each view against the
//! expected one. The native module's tests check the same files, so all three
//! show the same view for the same input. A plugin whose interpreter isn't
//! installed is skipped.

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::View;
use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

/// Every sample: `<name>.json` (the config) with `<name>.view.json`.
const SAMPLES: &[&str] = &["demo", "world", "hour12-seconds", "dst-start", "offset-now"];

fn repo(path: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect()
}

fn read(name: &str) -> serde_json::Value {
    let path = repo("crates/modules-fun/testdata/clock").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
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
            "clock-ts",
            vec!["node".into(), path("plugins/ext/clock-ts/main.ts")],
        ));
    }
    if have("python3") {
        out.push((
            "clock-py",
            vec!["python3".into(), path("plugins/ext/clock-py/main.py")],
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

#[tokio::test]
async fn plugins_match_the_expected_views() {
    for (id, argv) in plugins() {
        for name in SAMPLES {
            let mut plugin = Plugin::new("test", argv.clone());
            let manifest = plugin
                .init(read(&format!("{name}.json")))
                .await
                .expect("initialize");
            assert_eq!(manifest.id, id);
            assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
            let view = plugin.render().await.expect("view/render");
            assert_eq!(problems(check_view(&view)), Vec::<String>::new());
            let expected: View =
                serde_json::from_value(read(&format!("{name}.view.json"))).unwrap();
            assert_eq!(view, expected, "{id} on {name}");
        }
    }
}

#[tokio::test]
async fn plugins_show_local_time_by_default() {
    for (id, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv);
        plugin.init(json!({})).await.expect("initialize");
        let view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new(), "{id}");
        assert!(view.summary.starts_with("Local "), "{id}: {}", view.summary);
    }
}

#[tokio::test]
async fn plugins_reject_bad_settings() {
    // The native module rejects the same configs (see its tests).
    for (id, argv) in plugins() {
        for config in [
            json!({ "zones": ["Mars/Olympus_Mons"] }),
            json!({ "zones": ["+05:30"] }),
            json!({ "zones": ["America"] }),
            json!({ "zones": ["../zoneinfo/UTC"] }),
            json!({ "zones": [] }),
            json!({ "zones": [{ "zone": "UTC", "lable": "x" }] }),
            json!({ "zones": "UTC" }),
            json!({ "now": "yesterday" }),
            json!({ "now": "2026-02-29T00:00:00Z" }),
            json!({ "hour12": "yes" }),
            json!({ "zone": ["UTC"] }),
        ] {
            let err = Plugin::new("test", argv.clone())
                .init(config.clone())
                .await
                .unwrap_err();
            let err = format!("{err:#}");
            assert!(err.contains("code -32002"), "{id} on {config}: {err}");
        }
    }
}

#[test]
fn sdk_copies_match_the_sdk() {
    // Each package carries its own copy of the SDK, so it runs wherever it is
    // installed. Keep them in step with sdk/.
    for (copy, sdk) in [
        (
            "plugins/ext/clock-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/clock-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(repo(copy)).unwrap();
        let b = std::fs::read_to_string(repo(sdk)).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
