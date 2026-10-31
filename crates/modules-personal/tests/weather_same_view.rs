//! The built-in weather module and its TypeScript and Python copies in
//! `plugins/ext` must show the same view for the same Open-Meteo reply, so the
//! native-vs-plugin benchmark compares like with like. Each one calls a fake
//! Open-Meteo on 127.0.0.1 that sends recorded replies. A plugin whose
//! interpreter isn't installed is skipped.

#![cfg(all(unix, feature = "weather"))]

#[path = "common/open_meteo.rs"]
mod open_meteo;

use std::path::PathBuf;
use std::process::Command;

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

/// The view each implementation renders for `config`, as JSON.
async fn views(config: serde_json::Value) -> Vec<(String, serde_json::Value)> {
    let mut native = dashboard_modules_personal::weather::Weather::default();
    native.init(config.clone()).await.unwrap();
    let mut out = vec![(
        "native".to_string(),
        serde_json::to_value(native.render().await.unwrap()).unwrap(),
    )];
    for (runner, file) in [
        ("node", "plugins/ext/weather-ts/main.ts"),
        ("python3", "plugins/ext/weather-py/main.py"),
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

async fn same(extra: serde_json::Value) {
    let base = open_meteo::start().await;
    let all = views(open_meteo::config(&base, extra)).await;
    let (_, native) = &all[0];
    for (name, view) in &all[1..] {
        assert_eq!(view, native, "{name} differs from native");
    }
}

#[tokio::test]
async fn same_view_for_a_place() {
    same(json!({ "place": "Berlin" })).await;
}

#[tokio::test]
async fn same_view_for_coordinates_in_imperial_units() {
    same(json!({ "latitude": -33.8688, "longitude": 151.2093, "units": "imperial", "days": 3 }))
        .await;
}

#[tokio::test]
async fn same_view_with_a_name() {
    same(json!({ "place": "Berlin", "name": "Home" })).await;
}

/// A bad setting gets the same message from all three.
#[tokio::test]
async fn same_config_errors() {
    for (config, msg) in [
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
            json!({ "place": "Berlin", "days": -1 }),
            "days must be between 1 and 16",
        ),
        (
            json!({ "place": "Berlin", "days": 2.5 }),
            "days must be a whole number",
        ),
        (
            json!({ "place": "Berlin", "units": "kelvin" }),
            "units must be metric or imperial",
        ),
        (json!({ "place": 3 }), "place must be a string"),
        (
            json!({ "latitude": "1", "longitude": 2 }),
            "latitude must be a number",
        ),
        (json!({ "place": " " }), "place must not be empty"),
    ] {
        let err = dashboard_modules_personal::weather::Weather::default()
            .init(config.clone())
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), msg, "native");
        for (runner, file) in [
            ("node", "plugins/ext/weather-ts/main.ts"),
            ("python3", "plugins/ext/weather-py/main.py"),
        ] {
            if !have(runner) {
                continue;
            }
            let err = Plugin::new("test", vec![runner.into(), repo(file)])
                .init(config.clone())
                .await
                .unwrap_err();
            let err = format!("{err:#}");
            assert!(err.contains(msg), "{runner}: {err}");
        }
    }
}

/// Each package's demo is the built-in module's demo view.
#[test]
fn package_demos_match() {
    let native = serde_json::to_value(dashboard_modules_personal::weather::catalog().demo).unwrap();
    let found = dashboard_config::packages::Packages::load_from(&[repo("plugins/ext").into()]);
    assert!(found.errors.is_empty(), "{:?}", found.errors);
    for id in ["weather-ts", "weather-py"] {
        let package = &found.get(id).expect(id).package;
        let demo = serde_json::to_value(&package.demo).unwrap();
        assert_eq!(demo, native, "{id}");
    }
}

/// The built-in module fails with the same messages as the plugins.
#[tokio::test]
async fn native_errors() {
    let base = open_meteo::start().await;
    let mut w = dashboard_modules_personal::weather::Weather::default();
    w.init(open_meteo::config(&base, json!({ "place": "Nowhere" })))
        .await
        .unwrap();
    let err = w.render().await.unwrap_err().to_string();
    assert_eq!(err, "no place named \"Nowhere\"");

    let mut config = open_meteo::config(&base, json!({ "latitude": 1, "longitude": 2 }));
    config["forecast_url"] = format!("{base}/fail").into();
    let mut w = dashboard_modules_personal::weather::Weather::default();
    w.init(config).await.unwrap();
    let err = w.render().await.unwrap_err().to_string();
    assert!(
        err.starts_with("Open-Meteo: Latitude must be in range"),
        "{err}"
    );
}
