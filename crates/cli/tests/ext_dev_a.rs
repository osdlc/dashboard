//! Checks the GitLab, GitLab to-dos, Gerrit, Azure DevOps and CircleCI plugin
//! packages in `plugins/ext` (Python, token APIs, untested live). Each loads as
//! a package, its `--sample` render (sample.json, a hand-written API response)
//! is exactly its catalog demo view, its own `python3 -m unittest` parse tests
//! pass, a bad config is a config error, and with no token it says which
//! variable to set. None of these touch the network. Skipped when python3
//! isn't installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::{Packages, load_package};
use dashboard_contract::check::{Finding, check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

const IDS: [&str; 5] = [
    "gitlab",
    "gitlab-todos",
    "gerrit",
    "azure-devops",
    "circleci",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

fn plugin(id: &str, extra: &[&str]) -> Plugin {
    let p = load_package(&repo().join("plugins/ext").join(id)).unwrap();
    let mut argv = p.argv();
    argv.extend(extra.iter().map(|a| a.to_string()));
    Plugin::new(id, argv).in_dir(&p.dir)
}

#[test]
fn packages_load_with_metadata_and_the_current_sdk() {
    let sdk = std::fs::read_to_string(repo().join("sdk/python/dashboard_plugin.py")).unwrap();
    for id in IDS {
        let dir = repo().join("plugins/ext").join(id);
        let p = load_package(&dir).unwrap_or_else(|e| panic!("{id}: {e:#}"));
        let pkg = &p.package;
        assert!(
            pkg.description.contains("Untested live"),
            "{id}: token API packages say they are untested live"
        );
        assert!(pkg.tags.iter().any(|t| t == "token-api"), "{id}");
        assert!(pkg.config_schema.is_some(), "{id}");
        assert_eq!(pkg.env.len(), 1, "{id}: names its token variable");
        assert_eq!(
            problems(check_view(&pkg.demo)),
            Vec::<String>::new(),
            "{id}"
        );
        // The SDK is copied into each package; refresh the copies when it changes.
        let copy = std::fs::read_to_string(dir.join("dashboard_plugin.py")).unwrap();
        assert!(
            copy == sdk,
            "{id}: dashboard_plugin.py differs from sdk/python; copy it again"
        );
    }
}

#[tokio::test]
async fn sample_render_is_the_demo_view() {
    if !have_python() {
        return;
    }
    for id in IDS {
        let p = load_package(&repo().join("plugins/ext").join(id)).unwrap();
        let mut plugin = plugin(id, &["--sample"]);
        let manifest = plugin.init(json!(null)).await.expect("initialize");
        assert_eq!(manifest.id, id);
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
    for id in IDS {
        let out = Command::new("python3")
            .args(["-m", "unittest", "-q"])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(repo().join("plugins/ext").join(id))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{id}:\n{}{}",
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
        ("gitlab", json!({ "token_env": VAR })),
        ("gitlab-todos", json!({ "token_env": VAR })),
        (
            "gerrit",
            json!({ "url": "https://review.invalid", "username": "dana", "token_env": VAR }),
        ),
        (
            "azure-devops",
            json!({ "org": "acme", "project": "Web", "token_env": VAR }),
        ),
        (
            "circleci",
            json!({ "projects": ["gh/acme/api"], "token_env": VAR }),
        ),
    ];
    for (id, config) in configs {
        let mut plugin = plugin(id, &[]);
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
        ("gitlab", json!({ "url": "gitlab.com" })),
        (
            "gitlab",
            json!({ "sections": [{ "label": "x", "type": "pulls" }] }),
        ),
        ("gitlab-todos", json!({ "action": "nope" })),
        ("gitlab-todos", json!({ "limt": 5 })),
        ("gerrit", json!({})),
        (
            "gerrit",
            json!({ "url": "https://review.example.com", "auth": "basic" }),
        ),
        ("azure-devops", json!({ "org": "acme" })),
        (
            "azure-devops",
            json!({ "org": "acme", "project": "Web", "work_items": null, "pull_requests": false }),
        ),
        ("circleci", json!({})),
        ("circleci", json!({ "projects": ["acme/api"] })),
    ] {
        let err = plugin(id, &[]).init(config.clone()).await.unwrap_err();
        assert!(
            format!("{err:#}").contains("code -32002"),
            "{id} {config}: {err:#}"
        );
    }
}

#[test]
fn the_example_config_finds_every_package() {
    let path = repo().join("examples/widgets/w-dev-a.yaml");
    let loaded = dashboard_config::load(Some(&path)).unwrap();
    let packages = Packages::load(&path);
    for id in IDS {
        assert!(packages.get(id).is_some(), "{id} not found from {path:?}");
        assert!(
            loaded
                .config
                .modules
                .values()
                .any(|m| m.package.as_deref() == Some(id)),
            "{id} is not in the example"
        );
    }
}
