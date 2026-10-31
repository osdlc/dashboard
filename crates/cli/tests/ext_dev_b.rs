//! Checks the token-API plugin packages in `plugins/ext` (Buildkite, Jenkins,
//! Asana, Todoist, Trello): each loads as a package, its `--sample` render
//! (sample.json, a hand-written API response) is exactly its catalog demo view,
//! its own `python3 -m unittest` parse tests pass, with no token it says
//! which variable to set, and a failing `token_command` is reported without
//! its output. Skipped when python3 isn't installed. None of these
//! touch the network.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::load_package;
use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn ext_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext")
}

/// The token-API packages these tests cover.
const IDS: [&str; 5] = ["asana", "buildkite", "jenkins", "todoist", "trello"];

fn packages() -> Vec<PathBuf> {
    IDS.iter().map(|id| ext_dir().join(id)).collect()
}

fn have_python() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

fn problems(found: Vec<dashboard_contract::check::Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

#[test]
fn every_package_loads_and_carries_the_current_sdk() {
    let sdk = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/python/dashboard_plugin.py"),
    )
    .unwrap();
    let apitoken = std::fs::read_to_string(ext_dir().join("buildkite/apitoken.py")).unwrap();
    for dir in packages() {
        let p = load_package(&dir).unwrap_or_else(|e| panic!("{}: {e:#}", dir.display()));
        assert!(
            p.package.description.contains("Untested live"),
            "{}: token API packages say they are untested live",
            p.package.id
        );
        assert!(p.package.config_schema.is_some(), "{}", p.package.id);
        // The SDK is copied into each package; refresh the copies when it changes.
        let copy = std::fs::read_to_string(dir.join("dashboard_plugin.py")).unwrap();
        assert!(
            copy == sdk,
            "{}: dashboard_plugin.py differs from sdk/python; copy it again",
            p.package.id
        );
        // So is the token helper: a fix to one copy goes to all of them.
        let helper = std::fs::read_to_string(dir.join("apitoken.py")).unwrap();
        assert!(
            helper == apitoken,
            "{}: apitoken.py differs from plugins/ext/buildkite/apitoken.py; copy it again",
            p.package.id
        );
    }
}

#[tokio::test]
async fn sample_render_is_the_demo_view() {
    if !have_python() {
        return;
    }
    for dir in packages() {
        let p = load_package(&dir).unwrap();
        let id = &p.package.id;
        let mut argv = p.argv();
        argv.push("--sample".into());
        let mut plugin = Plugin::new(id, argv).in_dir(&p.dir);
        let manifest = plugin.init(json!(null)).await.expect("initialize");
        assert_eq!(&manifest.id, id);
        assert_eq!(manifest.name, p.package.name, "{id}");
        assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
        let view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new(), "{id}");
        assert_eq!(
            view, p.package.demo,
            "{id}: the demo in dashboard-plugin.yaml must match `python3 main.py --sample`"
        );
    }
}

#[test]
fn python_parse_tests_pass() {
    if !have_python() {
        return;
    }
    for dir in packages() {
        let out = Command::new("python3")
            .args(["-m", "unittest"])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}:\n{}{}",
            dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[tokio::test]
async fn without_a_token_it_names_the_variable() {
    if !have_python() {
        return;
    }
    const VAR: &str = "DASHBOARD_TEST_NO_SUCH_TOKEN";
    let configs = [
        ("asana", json!({ "workspace": "1", "token_env": VAR })),
        ("buildkite", json!({ "org": "acme", "token_env": VAR })),
        (
            "jenkins",
            json!({ "url": "https://ci.invalid", "user": "dana", "token_env": VAR }),
        ),
        ("todoist", json!({ "token_env": VAR })),
        (
            "trello",
            json!({ "board": "b", "key_env": VAR, "token_env": VAR }),
        ),
    ];
    for (id, config) in configs {
        let p = load_package(&ext_dir().join(id)).unwrap();
        let mut plugin = Plugin::new(id, p.argv()).in_dir(&p.dir);
        plugin.init(config).await.expect("initialize");
        let err = format!("{:#}", plugin.render().await.unwrap_err());
        assert!(err.contains("code -32001"), "{id}: {err}");
        assert!(err.contains(VAR), "{id}: {err}");
    }
}

#[tokio::test]
async fn bad_config_is_a_config_error() {
    if !have_python() {
        return;
    }
    for (id, config) in [
        ("buildkite", json!({})),
        ("jenkins", json!({ "url": "ci.example.com" })),
        ("asana", json!({})),
        ("trello", json!({ "board": "b", "limit": 0 })),
        ("todoist", json!({ "filtr": "today" })),
        // The host doesn't check config_schema, so the plugins check these types.
        (
            "todoist",
            json!({ "token_command": "op read op://dev/todoist" }),
        ),
        ("buildkite", json!({ "org": "acme", "token_env": 5 })),
        ("asana", json!({ "workspace": "1", "token_command": [] })),
        (
            "jenkins",
            json!({ "url": "https://ci.invalid", "token_command": ["op", 1] }),
        ),
        (
            "trello",
            json!({ "board": "b", "key_command": "op read op://dev/trello" }),
        ),
        ("trello", json!({ "board": "b", "key_env": "" })),
    ] {
        let p = load_package(&ext_dir().join(id)).unwrap();
        let err = Plugin::new(id, p.argv())
            .in_dir(&p.dir)
            .init(config)
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("code -32002"), "{id}: {err:#}");
    }
}

#[tokio::test]
async fn failing_token_command_hides_its_output() {
    if !have_python() {
        return;
    }
    let fail = json!(["sh", "-c", "echo s3cret; echo s3cret >&2; exit 1"]);
    let configs = [
        ("asana", json!({ "workspace": "1", "token_command": fail })),
        ("buildkite", json!({ "org": "acme", "token_command": fail })),
        (
            "jenkins",
            json!({ "url": "https://ci.invalid", "user": "dana", "token_command": fail }),
        ),
        ("todoist", json!({ "token_command": fail })),
        ("trello", json!({ "board": "b", "key_command": fail })),
    ];
    for (id, config) in configs {
        let p = load_package(&ext_dir().join(id)).unwrap();
        let mut plugin = Plugin::new(id, p.argv()).in_dir(&p.dir);
        plugin.init(config).await.expect("initialize");
        let err = format!("{:#}", plugin.render().await.unwrap_err());
        assert!(err.contains("code -32001"), "{id}: {err}");
        assert!(err.contains("failed (exit 1)"), "{id}: {err}");
        assert!(
            !err.contains("s3cret"),
            "{id}: the error shows the command's output: {err}"
        );
    }
}
