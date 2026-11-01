//! Runs the git benchmark plugins in `plugins/ext` (TypeScript and Python)
//! through the real plugin host against repositories made for the test, and
//! checks the exact view. The built-in `git` module must show the same; the
//! dashboard crate's `bench_git` test compares the three. A plugin whose
//! interpreter isn't installed is skipped. No network.

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_contract::View;
use dashboard_contract::check::{check_manifest, check_view};
use dashboard_host::{Module, Plugin};
use serde_json::json;

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

/// Node 24 or newer, as `runtime: node>=24` asks: older ones can't run
/// `main.ts` directly.
fn have_node() -> bool {
    let Ok(out) = Command::new("node").arg("--version").output() else {
        return false;
    };
    let version = String::from_utf8_lossy(&out.stdout);
    let major = version.trim().trim_start_matches('v').split('.').next();
    major
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|m| m >= 24)
}

/// (id, argv) for each implementation whose interpreter is installed.
fn plugins() -> Vec<(&'static str, Vec<String>)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/ext");
    let mut out = vec![];
    if have_node() {
        out.push((
            "git-ts",
            vec!["node".into(), format!("{root}/git-ts/main.ts")],
        ));
    }
    if have("python3") {
        out.push((
            "git-py",
            vec!["python3".into(), format!("{root}/git-py/git_py.py")],
        ));
    }
    out
}

/// Runs git without the user's own settings, which could change the counts.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("running git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A temporary folder, removed when dropped, also when the test fails.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `work`: a clone one commit ahead and one behind, with a changed and an
/// untracked file. `plain`: a clean repository with no upstream.
fn fixture(name: &str) -> TempDir {
    let root = std::env::temp_dir().join(format!("dashboard-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let origin = root.join("origin");
    let plain = root.join("plain");
    for dir in [&origin, &plain] {
        std::fs::create_dir(dir).unwrap();
        git(dir, &["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
    }
    git(&root, &["clone", "-q", "origin", "work"]);
    let work = root.join("work");
    std::fs::write(work.join("b.txt"), "b\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "ahead"]);
    std::fs::write(origin.join("c.txt"), "c\n").unwrap();
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-q", "-m", "behind"]);
    git(&work, &["fetch", "-q"]);
    std::fs::write(work.join("a.txt"), "changed\n").unwrap();
    std::fs::write(work.join("new.txt"), "new\n").unwrap();
    TempDir(root)
}

fn expected() -> View {
    let row = |repo: &str, branch: &str, changes: &str, sync: &str, status: &str| {
        json!({
            "cells": { "repo": repo, "branch": branch, "changes": changes, "sync": sync },
            "status": status,
        })
    };
    serde_json::from_value(json!({
        "status": "error",
        "summary": "3 repos · 1 dirty · 1 unpushed · 1 behind",
        "root": {
            "type": "table",
            "columns": [
                { "key": "repo", "label": "Repository" },
                { "key": "branch", "label": "Branch" },
                { "key": "changes", "label": "Changes" },
                { "key": "sync", "label": "Upstream" },
            ],
            "rows": [
                row("work", "main", "2 changed", "↑1 ↓1", "warn"),
                row("plain", "main", "clean", "no upstream", "ok"),
                row("missing", "-", "not a folder", "-", "error"),
            ],
        },
    }))
    .unwrap()
}

fn problems(found: Vec<dashboard_contract::check::Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

#[tokio::test]
async fn plugins_show_branch_sync_and_changes() {
    let tmp = fixture("bench-git-host");
    let root = &tmp.0;
    let repos: Vec<String> = ["work", "plain", "missing"]
        .iter()
        .map(|r| root.join(r).to_string_lossy().into_owned())
        .collect();
    for (id, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv.clone());
        let manifest = plugin
            .init(json!({ "repos": repos }))
            .await
            .expect("initialize");
        assert_eq!(manifest.id, id);
        assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
        let view = plugin.render().await.expect("view/render");
        assert_eq!(problems(check_view(&view)), Vec::<String>::new());
        assert_eq!(view, expected(), "{argv:?}");
    }
}

#[tokio::test]
async fn plugins_need_repos() {
    for (_, argv) in plugins() {
        for config in [json!({}), json!({ "repos": [] })] {
            let err = Plugin::new("test", argv.clone())
                .init(config)
                .await
                .unwrap_err();
            let err = format!("{err:#}");
            assert!(err.contains("code -32002"), "{argv:?}: {err}");
            assert!(err.contains("needs `config: { repos"), "{argv:?}: {err}");
        }
    }
}

#[tokio::test]
async fn plugins_need_paths_in_repos() {
    for (_, argv) in plugins() {
        let err = Plugin::new("test", argv.clone())
            .init(json!({ "repos": [1] }))
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("code -32002"), "{argv:?}: {err}");
        assert!(err.contains("list of paths"), "{argv:?}: {err}");
    }
}

/// `git status` can print more than 1 MB (Node's default limit for a child's
/// output): the plugins still show the row.
#[tokio::test]
async fn plugins_read_long_git_output() {
    let tmp = fixture("bench-git-long");
    let plain = tmp.0.join("plain");
    // About 1.3 MB of `? <name>` lines.
    let long = "x".repeat(250);
    for i in 0..5000 {
        std::fs::write(plain.join(format!("{long}{i}")), "").unwrap();
    }
    let repos = [plain.to_string_lossy().into_owned()];
    for (_, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv.clone());
        plugin
            .init(json!({ "repos": repos }))
            .await
            .expect("initialize");
        let view = plugin.render().await.expect("view/render");
        assert_eq!(view.summary, "1 repo · 1 dirty · 0 unpushed · 0 behind");
    }
}

/// Each package carries a copy of its SDK; it must match the one in `sdk/`.
#[test]
fn sdk_copies_are_current() {
    let repo = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    for (copy, sdk) in [
        (
            "plugins/ext/git-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/git-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(format!("{repo}/{copy}")).unwrap();
        let b = std::fs::read_to_string(format!("{repo}/{sdk}")).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
