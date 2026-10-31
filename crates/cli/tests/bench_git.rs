//! The git widget, three ways (native-vs-plugin benchmark): the built-in
//! `git` module and the TypeScript and Python plugins in `plugins/ext` must
//! show the same view for the same repositories, and the same demo view. A
//! plugin whose interpreter isn't installed is skipped. No network.
#![cfg(feature = "module-git")]

use std::path::{Path, PathBuf};
use std::process::Command;

use dashboard_config::packages::load_package;
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

#[tokio::test]
async fn native_and_plugins_show_the_same_view() {
    let tmp = fixture("bench-git-cli");
    let root = &tmp.0;
    let repos: Vec<String> = ["work", "plain", "missing"]
        .iter()
        .map(|r| root.join(r).to_string_lossy().into_owned())
        .collect();
    let config = json!({ "repos": repos });
    let mut native = dashboard_modules::builtin("git").expect("git module");
    native.init(config.clone()).await.expect("init");
    let want = native.render().await.expect("render");
    assert_eq!(want.summary, "3 repos · 1 dirty · 1 unpushed · 1 behind");
    for (_, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv.clone());
        plugin.init(config.clone()).await.expect("initialize");
        let view = plugin.render().await.expect("view/render");
        assert_eq!(view, want, "{argv:?}");
    }
}

#[test]
fn packages_have_the_native_demo_view() {
    let native = dashboard_modules::catalog()
        .into_iter()
        .find(|e| e.id == "git")
        .expect("git entry");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext");
    for id in ["git-ts", "git-py"] {
        let entry = load_package(&root.join(id)).expect(id).entry();
        assert_eq!(entry.demo, native.demo, "{id}");
        assert_eq!(
            entry.config_schema.as_ref().unwrap()["properties"]["repos"]["type"],
            "array"
        );
    }
}

/// A repository under a folder we can't read is "not a folder" in all three,
/// not a failed render. Skipped as root, who can read it anyway.
#[cfg(unix)]
#[tokio::test]
async fn unreadable_parent_is_not_a_folder() {
    use std::os::unix::fs::PermissionsExt;

    let uid = Command::new("id").arg("-u").output().expect("id -u");
    if String::from_utf8_lossy(&uid.stdout).trim() == "0" {
        return;
    }
    let root =
        std::env::temp_dir().join(format!("dashboard-bench-git-locked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let locked = root.join("locked");
    std::fs::create_dir_all(locked.join("repo")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    /// Gives the folder its permissions back so `TempDir` can remove it.
    struct Unlock(PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let _tmp = TempDir(root);
    let _unlock = Unlock(locked.clone());

    let config = json!({ "repos": [locked.join("repo").to_string_lossy()] });
    let mut native = dashboard_modules::builtin("git").expect("git module");
    native.init(config.clone()).await.expect("init");
    let want = native.render().await.expect("render");
    assert_eq!(want.summary, "1 repo · 0 dirty · 0 unpushed · 0 behind");
    for (_, argv) in plugins() {
        let mut plugin = Plugin::new("test", argv.clone());
        plugin.init(config.clone()).await.expect("initialize");
        let view = plugin.render().await.expect("view/render");
        assert_eq!(view, want, "{argv:?}");
    }
}
