//! The Stocks plugin package (`plugins/ext/stocks`, Python): run with `--sample`, it reads the
//! saved Yahoo Finance replies in `samples/` and must show the package's demo view. It also
//! checks its settings and the `sample` setting. Nothing here calls the network. Skipped
//! without python3.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::load_package;
use dashboard_contract::check::check_view;
use dashboard_contract::{Cell, Status, Widget};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext/stocks")
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

fn text(cell: &Cell) -> &str {
    match cell {
        Cell::Text(s) => s,
        other => panic!("not text: {other:?}"),
    }
}

#[tokio::test]
async fn sample_shows_the_demo_view() {
    let Some(argv) = argv(&["--sample"]) else {
        return;
    };
    let mut plugin = Plugin::new("stocks", argv);
    plugin.init(json!({})).await.expect("initialize");
    let view = plugin.render().await.expect("view/render");
    let problems: Vec<String> = check_view(&view)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    assert!(problems.is_empty(), "{problems:?}");

    assert_eq!(view.status, Status::Warn);
    assert_eq!(
        view.summary,
        "SPY 754.37 -0.4% · QQQ 679.89 -1.2% · AAPL 356.27 +0.4% · closed"
    );
    let Widget::Column { children } = &view.root else {
        panic!("column: {:?}", view.root);
    };
    let Widget::Table { rows, .. } = &children[0] else {
        panic!("table");
    };
    let symbols: Vec<&str> = rows.iter().map(|r| text(&r.cells["symbol"])).collect();
    assert_eq!(symbols, ["SPY", "QQQ", "AAPL", "MSFT", "NVDA"]);
    // NVDA fell 2.0% from the previous close.
    assert_eq!(rows[4].status, Some(Status::Warn));
    assert_eq!(rows[0].status, None);
    assert!(matches!(children[1], Widget::Series { .. }));

    let package = load_package(&dir()).expect("package loads");
    assert_eq!(package.package.demo, view, "demo: in dashboard-plugin.yaml");
}

#[tokio::test]
async fn settings_change_the_view() {
    let Some(argv) = argv(&["--sample"]) else {
        return;
    };
    let mut plugin = Plugin::new("stocks", argv);
    plugin
        .init(json!({ "symbols": ["aapl", "NOSUCHSYMBOLXX"], "chart": false }))
        .await
        .expect("initialize");
    let view = plugin.render().await.expect("view/render");
    // No chart: the table alone. The unknown symbol is an error row, so the tile warns.
    let Widget::Table { rows, .. } = &view.root else {
        panic!("table: {:?}", view.root);
    };
    assert_eq!(text(&rows[0].cells["symbol"]), "AAPL");
    assert_eq!(rows[1].status, Some(Status::Error));
    assert_eq!(text(&rows[1].cells["price"]), "not found");
    assert_eq!(view.status, Status::Warn);
}

#[tokio::test]
async fn bad_settings() {
    let Some(argv) = argv(&[]) else {
        return;
    };
    for (config, msg) in [
        (json!({ "symbols": [] }), "symbols"),
        (json!({ "symbols": ["not a symbol"] }), "symbols"),
        (json!({ "warn_drop": 0 }), "warn_drop"),
        (json!({ "chart": "yes" }), "chart"),
        (json!({ "timeout": 0 }), "timeout"),
        (json!({ "colour": "red" }), "colour"),
    ] {
        let mut plugin = Plugin::new("stocks", argv.clone());
        let err = format!("{:#}", plugin.init(config).await.unwrap_err());
        assert!(err.contains(msg), "{err}");
    }
}

#[test]
fn python_parse_tests_pass() {
    if argv(&[]).is_none() {
        return;
    }
    let out = Command::new("python3")
        .args(["-B", "-m", "unittest", "-q"])
        .current_dir(dir())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The package carries a copy of the SDK; it must match the one in `sdk/`.
#[test]
fn sdk_copy_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let copy = std::fs::read_to_string(dir().join("dashboard_plugin.py")).unwrap();
    let sdk = std::fs::read_to_string(root.join("sdk/python/dashboard_plugin.py")).unwrap();
    assert!(copy == sdk, "copy sdk/python/dashboard_plugin.py again");
}
