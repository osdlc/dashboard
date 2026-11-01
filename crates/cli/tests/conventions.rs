//! Exit codes and `--json` documents of the built binary (docs/cli.md).

use std::path::PathBuf;
use std::process::{Command, Output};

/// Run `dashboard` with its dirs inside a fresh temp dir, so no user config or
/// server record is touched.
fn dashboard(args: &[&str]) -> Output {
    let home: PathBuf = std::env::temp_dir().join(format!(
        "dashboard-conventions-{}-{}",
        std::process::id(),
        args.join("-").replace(['/', ' '], "_")
    ));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dashboard"));
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
    ] {
        cmd.env(var, home.join(var));
    }
    let out = cmd.args(args).output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    out
}

fn document(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(&out.stdout));
    })
}

#[test]
fn usage_errors_exit_2_with_an_error_document() {
    let out = dashboard(&["nosuch", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let doc = document(&out);
    assert_eq!(doc["schemaVersion"], 1);
    assert_eq!(doc["kind"], "error");
    assert_eq!(doc["code"], "usage");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("dashboard: "), "{err}");

    // Without --json: the line only.
    let out = dashboard(&["serve", "--port", "x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn serve_refuses_json() {
    let out = dashboard(&["serve", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(document(&out)["code"], "usage");
}

#[test]
fn status_and_open_without_a_server() {
    let out = dashboard(&["status", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let doc = document(&out);
    assert_eq!(doc["kind"], "status");
    assert_eq!(doc["running"], false);

    let out = dashboard(&["open", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(document(&out)["code"], "not-running");
}

#[test]
fn a_missing_plugin_program_exits_22() {
    let out = dashboard(&["plugin-check", "--", "/nonexistent/dashboard-plugin"]);
    assert_eq!(out.status.code(), Some(22));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("dashboard: couldn't start the plugin"),
        "{err}"
    );
}
