//! The Healthchecks.io plugin package (`plugins/ext/healthchecks`, Python):
//! run with `--sample`, it reads its hand-written sample reply and must show
//! the package's demo view. It also checks its settings, and never calls the
//! API here: with no key it fails before any request. Skipped without
//! python3.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::load_package;
use dashboard_contract::check::check_view;
use dashboard_contract::{Status, Widget};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext/healthchecks")
}

fn argv(extra: &[&str]) -> Option<Vec<String>> {
    Command::new("python3").arg("--version").output().ok()?;
    let mut argv = vec![
        "python3".to_string(),
        dir().join("main.py").display().to_string(),
    ];
    argv.extend(extra.iter().map(|s| s.to_string()));
    Some(argv)
}

#[tokio::test]
async fn sample_shows_the_demo_view() {
    let Some(argv) = argv(&["--sample"]) else {
        return;
    };
    let mut plugin = Plugin::new("healthchecks", argv);
    plugin.init(json!({})).await.expect("initialize");
    let view = plugin.render().await.expect("view/render");
    let problems: Vec<String> = check_view(&view)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    assert!(problems.is_empty(), "{problems:?}");

    assert_eq!(view.status, Status::Error);
    assert_eq!(view.summary, "1 down · 1 late · 5 checks");
    let Widget::Column { children } = &view.root else {
        panic!("column: {:?}", view.root);
    };
    let Widget::List { items, .. } = &children[1] else {
        panic!("list");
    };
    // Down first, then late; a check with no name shows its slug.
    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Nightly backup",
            "Certificate renewal",
            "Queue worker heartbeat",
            "new-check",
            "Staging smoke test"
        ]
    );
    // "Staging smoke test" has `unique_key` and no `uuid`, as a read-only API
    // key returns it, so its link falls back to the site's front page.
    assert_eq!(
        items[4].action,
        Some(dashboard_contract::Action::OpenUrl {
            url: "https://healthchecks.io/".into()
        })
    );

    let package = load_package(&dir()).expect("package loads");
    assert_eq!(package.package.demo, view, "demo: in dashboard-plugin.yaml");
}

#[tokio::test]
async fn limit_and_self_hosted_links() {
    let Some(argv) = argv(&["--sample"]) else {
        return;
    };
    let mut plugin = Plugin::new("healthchecks", argv);
    plugin
        .init(json!({ "base_url": "https://hc.example.com/", "limit": 2 }))
        .await
        .expect("initialize");
    let view = plugin.render().await.expect("view/render");
    let Widget::Column { children } = &view.root else {
        panic!()
    };
    let Widget::List { items, .. } = &children[1] else {
        panic!()
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].action,
        Some(dashboard_contract::Action::OpenUrl {
            url: "https://hc.example.com/checks/5d3b6e2a-1f4c-4a9e-9b1d-0c2e7f8a6b31/details/"
                .into()
        })
    );
}

#[tokio::test]
async fn bad_settings_and_no_key() {
    let Some(argv) = argv(&[]) else {
        return;
    };
    for (config, msg) in [
        (json!({ "base_url": "healthchecks.io" }), "base_url"),
        (json!({ "limit": 0 }), "limit"),
        (json!({ "token_command": "pass hc" }), "token_command"),
        (json!({ "colour": "red" }), "colour"),
    ] {
        let mut plugin = Plugin::new("healthchecks", argv.clone());
        let err = format!("{:#}", plugin.init(config).await.unwrap_err());
        assert!(err.contains(msg), "{err}");
    }

    // An unset variable: an error before any request is made.
    let mut plugin = Plugin::new("healthchecks", argv);
    plugin
        .init(json!({ "token_env": "DASHBOARD_TEST_NO_SUCH_VAR" }))
        .await
        .expect("initialize");
    let err = format!("{:#}", plugin.render().await.unwrap_err());
    assert!(err.contains("$DASHBOARD_TEST_NO_SUCH_VAR"), "{err}");
}

/// The package carries a copy of the SDK; it must match the one in `sdk/`.
#[test]
fn sdk_copy_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let copy = std::fs::read_to_string(dir().join("dashboard_plugin.py")).unwrap();
    let sdk = std::fs::read_to_string(root.join("sdk/python/dashboard_plugin.py")).unwrap();
    assert!(copy == sdk, "copy sdk/python/dashboard_plugin.py again");
}
