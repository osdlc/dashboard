//! Checks the ops token-API plugin packages in `plugins/ext` (Airbrake,
//! DigitalOcean, Rollbar, UptimeRobot, Zendesk). Each loads
//! as a package, its `--sample` render is exactly its catalog demo view, its
//! own parse tests (`python3 -m unittest`) pass, a bad config is a config error
//! and, with no token, the error names the variable to set. Nothing here calls
//! the network. Skipped when python3 isn't installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::load_package;
use dashboard_contract::check::{Finding, check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn ext_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext")
}

/// The packages these tests cover, by name. Other token-API packages in
/// plugins/ext have tests of their own.
const IDS: [&str; 5] = [
    "airbrake",
    "digitalocean",
    "rollbar",
    "uptimerobot",
    "zendesk",
];

fn packages() -> Vec<PathBuf> {
    IDS.iter().map(|id| ext_dir().join(id)).collect()
}

fn have_python() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

fn problems(found: Vec<Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

#[test]
fn every_package_loads_and_carries_the_shared_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sdk = std::fs::read_to_string(root.join("sdk/python/dashboard_plugin.py")).unwrap();
    let dirs = packages();
    let helpers = std::fs::read_to_string(dirs[0].join("token_api.py")).unwrap();
    let apitoken = std::fs::read_to_string(dirs[0].join("apitoken.py")).unwrap();
    let apitoken_test = std::fs::read_to_string(dirs[0].join("test_apitoken.py")).unwrap();
    // apitoken.py is shared with the dev-b packages too; once Buildkite is
    // here, its copy is the reference, as in ext_dev_b.rs.
    if let Ok(reference) = std::fs::read_to_string(ext_dir().join("buildkite/apitoken.py")) {
        assert!(
            apitoken == reference,
            "apitoken.py differs from plugins/ext/buildkite/apitoken.py; copy it again"
        );
    }
    for dir in dirs {
        let p = load_package(&dir).unwrap_or_else(|e| panic!("{}: {e:#}", dir.display()));
        let id = &p.package.id;
        assert_eq!(p.package.run.first().map(String::as_str), Some("python3"));
        assert!(
            p.package.description.contains("Untested live"),
            "{id}: token API packages say they are untested live"
        );
        assert!(p.package.config_schema.is_some(), "{id}");
        // Each package carries copies of the SDK, apitoken.py (and its
        // test) and token_api.py; they must stay the same everywhere.
        let copy = std::fs::read_to_string(dir.join("dashboard_plugin.py")).unwrap();
        assert!(
            copy == sdk,
            "{id}: dashboard_plugin.py differs from sdk/python; copy it again"
        );
        let copy = std::fs::read_to_string(dir.join("token_api.py")).unwrap();
        assert!(
            copy == helpers,
            "{id}: token_api.py differs from the other packages' copies"
        );
        let copy = std::fs::read_to_string(dir.join("apitoken.py")).unwrap();
        assert!(
            copy == apitoken,
            "{id}: apitoken.py differs from the other packages' copies"
        );
        let copy = std::fs::read_to_string(dir.join("test_apitoken.py")).unwrap();
        assert!(
            copy == apitoken_test,
            "{id}: test_apitoken.py differs from the other packages' copies"
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
            .args(["-B", "-m", "unittest", "-q"])
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

/// A config each package accepts, with its token read from `token_env`.
fn good_config(id: &str, token_env: &str) -> serde_json::Value {
    match id {
        "airbrake" => json!({ "project_id": 1, "token_env": token_env }),
        "rollbar" => json!({ "account": "acme", "project": "web", "token_env": token_env }),
        "zendesk" => {
            json!({ "subdomain": "acme", "email": "me@acme.test", "token_env": token_env })
        }
        "digitalocean" | "uptimerobot" => json!({ "token_env": token_env }),
        _ => unreachable!("add a config for {id}"),
    }
}

#[tokio::test]
async fn without_a_token_it_names_the_variable() {
    if !have_python() {
        return;
    }
    const VAR: &str = "DASHBOARD_TEST_NO_SUCH_TOKEN";
    for dir in packages() {
        let p = load_package(&dir).unwrap();
        let id = p.package.id.as_str();
        let mut plugin = Plugin::new(id, p.argv()).in_dir(&p.dir);
        plugin.init(good_config(id, VAR)).await.expect("initialize");
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
    for dir in packages() {
        let p = load_package(&dir).unwrap();
        let id = p.package.id.as_str();
        // A typo, and a limit out of range.
        let mut typo = good_config(id, "X");
        typo["limt"] = json!(5);
        let mut limit = good_config(id, "X");
        limit["limit"] = json!(0);
        for config in [typo, limit] {
            let err = Plugin::new(id, p.argv())
                .in_dir(&p.dir)
                .init(config.clone())
                .await
                .unwrap_err();
            assert!(
                format!("{err:#}").contains("code -32002"),
                "{id} {config}: {err:#}"
            );
        }
    }
}
