//! Runs the TypeScript and Python weather plugins in `plugins/ext` (the
//! benchmark copies of the built-in `weather` module) through the real plugin
//! host. They call a fake Open-Meteo on 127.0.0.1 that sends recorded replies,
//! so no network is needed. A plugin whose interpreter isn't installed is
//! skipped. The test that all three build the same view is in
//! `crates/modules-personal/tests`.

#![cfg(unix)]

#[path = "../../modules-personal/tests/common/open_meteo.rs"]
mod open_meteo;

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

/// Whether `program` is installed. Node must be 24 or newer, the packages'
/// `runtime:`, since older versions don't run .ts files without a flag.
fn have(program: &str) -> bool {
    let Ok(out) = Command::new(program).arg("--version").output() else {
        return false;
    };
    if program != "node" {
        return true;
    }
    let version = String::from_utf8_lossy(&out.stdout);
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 24)
}

/// Both plugins whose interpreter is installed.
fn plugins() -> Vec<Vec<String>> {
    let mut out = vec![];
    if have("node") {
        out.push(vec!["node".into(), repo("plugins/ext/weather-ts/main.ts")]);
    }
    if have("python3") {
        out.push(vec![
            "python3".into(),
            repo("plugins/ext/weather-py/main.py"),
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

/// The error a render gives, as text.
async fn render_error(argv: &[String], config: serde_json::Value) -> String {
    let mut plugin = Plugin::new("test", argv.to_vec());
    plugin.init(config).await.expect("initialize");
    format!("{:#}", plugin.render().await.unwrap_err())
}

#[tokio::test]
async fn shows_now_and_the_forecast_for_a_place() {
    let base = open_meteo::start().await;
    for argv in plugins() {
        let view = render(
            &argv,
            open_meteo::config(&base, json!({ "place": "Berlin" })),
        )
        .await;
        assert_eq!(view.status, Status::Ok, "{argv:?}");
        assert_eq!(view.summary, "Berlin · 10°C · Overcast", "{argv:?}");
        let Widget::Column { children } = &view.root else {
            panic!("{argv:?}: column")
        };
        let Widget::Table { rows, .. } = &children[1] else {
            panic!("{argv:?}: table")
        };
        assert_eq!(rows.len(), 5, "{argv:?}");
        assert_eq!(
            rows[1].cells["day"],
            Cell::Text("Wed 28".into()),
            "{argv:?}"
        );
        assert_eq!(rows[3].cells["low"], Cell::Text("-3°".into()), "{argv:?}");
        assert_eq!(rows[2].status, Some(Status::Warn), "{argv:?}");
    }
}

#[tokio::test]
async fn coordinates_and_imperial_units() {
    let base = open_meteo::start().await;
    for argv in plugins() {
        let config = json!({ "latitude": 40.7128, "longitude": -74.006, "units": "imperial" });
        let view = render(&argv, open_meteo::config(&base, config)).await;
        assert_eq!(view.status, Status::Warn, "{argv:?}");
        assert_eq!(
            view.summary, "40.71, -74.01 · 50°F · Thunderstorm, hail",
            "{argv:?}"
        );
    }
}

#[tokio::test]
async fn unknown_place_and_api_errors() {
    let base = open_meteo::start().await;
    for argv in plugins() {
        let err = render_error(
            &argv,
            open_meteo::config(&base, json!({ "place": "Nowhere" })),
        )
        .await;
        assert!(err.contains("code -32001"), "{argv:?}: {err}");
        assert!(
            err.contains("no place named \"Nowhere\""),
            "{argv:?}: {err}"
        );

        let mut config = open_meteo::config(&base, json!({ "latitude": 1, "longitude": 2 }));
        config["forecast_url"] = format!("{base}/fail").into();
        let err = render_error(&argv, config).await;
        assert!(
            err.contains("Open-Meteo: Latitude must be in range"),
            "{argv:?}: {err}"
        );
    }
}

#[tokio::test]
async fn rejects_bad_settings() {
    for argv in plugins() {
        for (config, msg) in [
            (json!({ "palce": "Berlin" }), "palce"),
            (json!({}), "set place, or latitude and longitude"),
            (
                json!({ "place": "Berlin", "longitude": 3 }),
                "set place, or latitude and longitude, not both",
            ),
            (
                json!({ "latitude": 1, "longitude": 200 }),
                "longitude must be between -180 and 180",
            ),
            (
                json!({ "place": "Berlin", "days": 17 }),
                "days must be between 1 and 16",
            ),
            (
                json!({ "place": "Berlin", "units": "kelvin" }),
                "units must be metric or imperial",
            ),
            (
                json!({ "place": "Berlin", "days": 2.5 }),
                "days must be a whole number",
            ),
            (json!({ "place": " " }), "place must not be empty"),
        ] {
            let err = Plugin::new("test", argv.clone())
                .init(config)
                .await
                .unwrap_err();
            let err = format!("{err:#}");
            assert!(err.contains("code -32002"), "{argv:?}: {err}");
            assert!(err.contains(msg), "{argv:?}: {err}");
        }
    }
}

/// Each package carries a copy of its SDK; it must match the one in `sdk/`.
#[test]
fn sdk_copies_are_current() {
    for (copy, sdk) in [
        (
            "plugins/ext/weather-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/weather-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(repo(copy)).unwrap();
        let b = std::fs::read_to_string(repo(sdk)).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
