//! The containers plugin package (`plugins/ext/containers`, Python): VMs and
//! Docker engines read with the colima, limactl, podman and docker CLIs. Here
//! those are fake scripts in the package's `testdata/`, so no real runtime is
//! needed. Checks the demo view, each source being skipped or failing, a missing
//! socket, `show_exited`, `limit` and bad settings. Skipped without python3.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::load_package;
use dashboard_contract::check::check_view;
use dashboard_contract::{Status, View};
use dashboard_host::{Module, Plugin};
use serde_json::{Value, json};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext/containers")
}

fn have_python() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

fn fake(folder: &str, program: &str) -> String {
    dir()
        .join("testdata")
        .join(folder)
        .join(program)
        .display()
        .to_string()
}

/// A config with all four programs the fake ones in `testdata/ok`.
fn ok_config() -> Value {
    json!({
        "docker": fake("ok", "docker"),
        "colima": fake("ok", "colima"),
        "limactl": fake("ok", "limactl"),
        "podman": fake("ok", "podman"),
    })
}

fn plugin() -> Plugin {
    Plugin::new(
        "containers",
        vec![
            "python3".to_string(),
            dir().join("main.py").display().to_string(),
        ],
    )
}

async fn render(config: Value) -> View {
    let mut plugin = plugin();
    plugin.init(config).await.expect("initialize");
    let view = plugin.render().await.expect("view/render");
    let problems: Vec<String> = check_view(&view)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
    view
}

/// The cell values of every table row, as one string per row.
fn rows(view: &View) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                if let Some(Value::Object(cells)) = o.get("cells") {
                    let line: Vec<&str> = cells.values().filter_map(Value::as_str).collect();
                    out.push(line.join("|"));
                }
                o.values().for_each(|v| walk(v, out));
            }
            Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&serde_json::to_value(view).unwrap(), &mut out);
    out
}

/// Whether one row holds every `&`-separated part of `needle`.
fn has_row(view: &View, needle: &str) -> bool {
    rows(view)
        .iter()
        .any(|r| needle.split('&').all(|part| r.contains(part)))
}

#[tokio::test]
async fn everything_present_is_the_demo_view() {
    if !have_python() {
        return;
    }
    let view = render(ok_config()).await;
    assert_eq!(view.status, Status::Ok);
    assert_eq!(view.summary, "2/4 VMs up · 2/3 engines · 3 containers");
    for needle in [
        "colima&default&Running&4 · 8 GiB · 100 GiB&docker",
        "lima&dev&Running",
        "podman&podman-machine-default&Stopped",
        "desktop&not running",
        "postgres",
        "web",
    ] {
        assert!(has_row(&view, needle), "{needle}: {:?}", rows(&view));
    }
    assert!(!has_row(&view, "old-build"));

    let package = load_package(&dir()).expect("package loads");
    assert_eq!(package.package.demo, view, "demo: in dashboard-plugin.yaml");
    let mut plugin = plugin();
    let manifest = plugin.init(ok_config()).await.expect("initialize");
    assert_eq!(manifest.name, package.package.name);
    assert_eq!(manifest.id, package.package.id);
}

#[tokio::test]
async fn a_missing_or_disabled_cli_is_skipped() {
    if !have_python() {
        return;
    }
    let mut config = ok_config();
    config["podman"] = json!(fake("ok", "no-such-podman"));
    config["limactl"] = json!("");
    config["colima"] = json!(false);
    let view = render(config).await;
    assert_eq!(view.status, Status::Ok);
    assert_eq!(view.summary, "0/0 VMs up · 2/3 engines · 3 containers");
    assert!(!has_row(&view, "podman"));
    assert!(!has_row(&view, "lima&dev"));
    assert!(!has_row(&view, "colima&default"));

    // No docker either: nothing is found, which is not an error.
    let view = render(json!({
        "docker": false, "colima": "", "limactl": "", "podman": ""
    }))
    .await;
    assert_eq!(view.status, Status::Ok);
    assert_eq!(view.summary, "0/0 VMs up · 0/0 engines · 0 containers");
}

#[tokio::test]
async fn a_failing_cli_is_an_error_row_and_warns() {
    if !have_python() {
        return;
    }
    let mut config = ok_config();
    config["limactl"] = json!(fake("limactl-fails", "limactl"));
    let view = render(config).await;
    assert_eq!(view.status, Status::Warn);
    assert!(
        has_row(&view, "lima&limactl: fatal: instance store is corrupt"),
        "{:?}",
        rows(&view)
    );
    // The other sources are unaffected.
    assert!(has_row(&view, "colima&default&Running"));
    assert!(has_row(&view, "postgres"));

    let mut config = ok_config();
    config["docker"] = json!(fake("docker-down", "docker"));
    let view = render(config).await;
    assert_eq!(view.status, Status::Warn);
    assert!(
        has_row(&view, "docker&Cannot connect to the Docker daemon"),
        "{:?}",
        rows(&view)
    );
    assert!(has_row(&view, "colima&default&Running"));
}

#[tokio::test]
async fn one_unreachable_engine_warns_and_the_rest_show() {
    if !have_python() {
        return;
    }
    let mut config = ok_config();
    config["docker"] = json!(fake("docker-flaky", "docker"));
    let view = render(config).await;
    assert_eq!(view.status, Status::Warn);
    assert_eq!(view.summary, "2/4 VMs up · 1/2 engines · 2 containers");
    assert!(has_row(&view, "remote&Cannot connect to the Docker daemon"));
    assert!(has_row(&view, "postgres"));
}

#[tokio::test]
async fn a_unix_socket_that_exists_is_asked_for_its_containers() {
    if !have_python() {
        return;
    }
    // A context whose socket file exists is queried; the fake docker answers
    // with the containers of the context named `colima`.
    let tmp = std::env::temp_dir().join(format!("dashboard-containers-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let socket = tmp.join("docker.sock");
    std::fs::write(&socket, "").unwrap();
    let contexts = tmp.join("contexts.jsonl");
    std::fs::write(
        &contexts,
        format!(
            "{{\"Current\":true,\"DockerEndpoint\":\"unix://{}\",\"Error\":\"\",\"Name\":\"colima\"}}\n",
            socket.display()
        ),
    )
    .unwrap();
    // A docker that lists that context and otherwise behaves like the fake one.
    let script = tmp.join("docker");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = context ]; then cat '{}'; exit 0; fi\nexec '{}' \"$@\"\n",
            contexts.display(),
            fake("ok", "docker")
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut config = ok_config();
    config["docker"] = json!(script.display().to_string());
    let view = render(config).await;
    std::fs::remove_dir_all(&tmp).ok();
    assert_eq!(view.summary, "2/4 VMs up · 1/1 engines · 2 containers");
    assert!(has_row(&view, "postgres"));
}

#[tokio::test]
async fn show_exited_and_limit() {
    if !have_python() {
        return;
    }
    let mut config = ok_config();
    config["show_exited"] = json!(true);
    let view = render(config).await;
    assert!(has_row(&view, "old-build"), "{:?}", rows(&view));
    // Exited containers are listed but do not count as running.
    assert_eq!(view.summary, "2/4 VMs up · 2/3 engines · 3 containers");

    let mut config = ok_config();
    config["limit"] = json!(1);
    let view = render(config).await;
    assert!(has_row(&view, "cache"));
    assert!(!has_row(&view, "postgres"));
    assert!(!has_row(&view, "web"));
    assert_eq!(view.summary, "2/4 VMs up · 2/3 engines · 3 containers");
}

#[tokio::test]
async fn show_stopped_vms_false_hides_them() {
    if !have_python() {
        return;
    }
    let mut config = ok_config();
    config["show_stopped_vms"] = json!(false);
    let view = render(config).await;
    assert!(has_row(&view, "colima&default&Running"));
    assert!(!has_row(&view, "colima&staging&Stopped"));
    assert!(!has_row(&view, "podman-machine-default"));
}

#[tokio::test]
async fn bad_settings_are_config_errors() {
    if !have_python() {
        return;
    }
    for (config, msg) in [
        (json!({ "limit": -1 }), "limit"),
        (json!({ "limit": "ten" }), "limit"),
        (json!({ "timeout": 0 }), "timeout"),
        (json!({ "show_exited": "yes" }), "show_exited"),
        (json!({ "docker": true }), "docker"),
        (json!({ "podman": 3 }), "podman"),
        (json!({ "colour": "red" }), "colour"),
    ] {
        let mut plugin = plugin();
        let err = format!("{:#}", plugin.init(config).await.unwrap_err());
        assert!(err.contains(msg), "{err}");
    }
}

#[test]
fn python_unit_tests_pass() {
    if !have_python() {
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
