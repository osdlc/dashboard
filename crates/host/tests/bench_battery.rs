//! Runs the TypeScript and Python battery plugins (`plugins/ext/battery-ts`,
//! `plugins/ext/battery-py`) through the real plugin host on the samples in
//! `crates/modules-system/testdata/battery`, and checks each view against the
//! expected one. The native module's tests check the same files, so all three
//! show the same view for the same input. A plugin whose interpreter isn't
//! installed is skipped.

use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::View;
use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

/// Every sample: `<name>.txt` (pmset output) or the folder `<name>` (sysfs).
const SAMPLES: &[&str] = &[
    "pmset-ac",
    "pmset-charging",
    "pmset-desktop",
    "pmset-discharging",
    "pmset-finishing",
    "pmset-low",
    "pmset-ups",
    "sysfs-charging",
    "sysfs-desktop",
    "sysfs-discharging",
    "sysfs-negative-rate",
    "sysfs-peripheral-first",
];

fn repo(path: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect()
}

fn data(name: &str) -> PathBuf {
    repo("crates/modules-system/testdata/battery").join(name)
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
            "battery-ts",
            vec!["node".into(), path("plugins/ext/battery-ts/main.ts")],
        ));
    }
    if have("python3") {
        out.push((
            "battery-py",
            vec!["python3".into(), path("plugins/ext/battery-py/main.py")],
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
            let sample = match data(name) {
                dir if dir.is_dir() => dir,
                _ => data(&format!("{name}.txt")),
            };
            let mut plugin = Plugin::new("test", argv.clone());
            let manifest = plugin
                .init(json!({ "sample": sample }))
                .await
                .expect("initialize");
            assert_eq!(manifest.id, id);
            assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
            let view = plugin.render().await.expect("view/render");
            assert_eq!(problems(check_view(&view)), Vec::<String>::new());
            let text = std::fs::read_to_string(data(&format!("{name}.view.json"))).unwrap();
            let expected: View = serde_json::from_str(&text).unwrap();
            assert_eq!(view, expected, "{id} on {name}");
        }
    }
}

#[tokio::test]
async fn plugins_render_this_machine() {
    // Whatever this machine has (a battery, AC only, or no power_supply folder),
    // the plugin answers with a valid view.
    for (id, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv);
        plugin.init(json!({})).await.expect("initialize");
        let view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new(), "{id}");
    }
}

#[tokio::test]
async fn plugins_reject_unknown_settings() {
    for (id, argv) in plugins() {
        let err = Plugin::new("test", argv)
            .init(json!({ "lwo": 5 }))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("code -32002"), "{id}: {err}");
        assert!(err.contains("lwo"), "{id}: {err}");
    }
}

#[tokio::test]
async fn a_missing_sample_is_an_upstream_error() {
    for (id, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv);
        plugin
            .init(json!({ "sample": "/no/such/file" }))
            .await
            .expect("initialize");
        let err = format!("{:#}", plugin.render().await.unwrap_err());
        assert!(err.contains("code -32001"), "{id}: {err}");
    }
}

#[test]
fn sdk_copies_match_the_sdk() {
    // Each package carries its own copy of the SDK, so it runs wherever it is
    // installed. Keep them in step with sdk/.
    for (copy, sdk) in [
        (
            "plugins/ext/battery-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/battery-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(repo(copy)).unwrap();
        let b = std::fs::read_to_string(repo(sdk)).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
