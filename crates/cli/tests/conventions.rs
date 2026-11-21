//! Exit codes and `--json` documents of the built binary (docs/cli.md).

use std::path::PathBuf;
use std::process::{Command, Output};

/// Run `sitroom` with its dirs inside a fresh temp dir, so no user config or
/// server record is touched.
fn sitroom(args: &[&str]) -> Output {
    let home: PathBuf = std::env::temp_dir().join(format!(
        "sitroom-conventions-{}-{}",
        std::process::id(),
        args.join("-").replace(['/', ' '], "_")
    ));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_sitroom"));
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
    let out = sitroom(&["nosuch", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let doc = document(&out);
    assert_eq!(doc["schemaVersion"], 1);
    assert_eq!(doc["kind"], "error");
    assert_eq!(doc["code"], "usage");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("sitroom: "), "{err}");

    // Without --json: the line only.
    let out = sitroom(&["serve", "--port", "x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn version_prints_the_crate_version() {
    for flag in ["--version", "-V"] {
        let out = sitroom(&[flag]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("sitroom {}", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn help_lists_every_exit_code() {
    let out = sitroom(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8_lossy(&out.stdout);
    for code in [0, 1, 2, 22, 23, 24, 25] {
        assert!(help.contains(&format!(" {code} ")), "{code}: {help}");
    }
}

#[test]
fn serve_refuses_json() {
    let out = sitroom(&["serve", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(document(&out)["code"], "usage");
}

#[test]
fn status_and_open_without_a_server() {
    let out = sitroom(&["status", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let doc = document(&out);
    assert_eq!(doc["kind"], "status");
    assert_eq!(doc["running"], false);

    let out = sitroom(&["open", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(document(&out)["code"], "not-running");
}

#[test]
fn a_missing_plugin_program_exits_22() {
    let out = sitroom(&["plugin-check", "--", "/nonexistent/sitroom-plugin"]);
    assert_eq!(out.status.code(), Some(22));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("sitroom: couldn't start the plugin"),
        "{err}"
    );
}
