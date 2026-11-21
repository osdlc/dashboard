//! `sitroom import`, `dashboards list`, `export` and `config` with layers,
//! run as a user would, with the config and data homes and the working
//! directory in a temp folder. No network.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("sitroom-cli-dashboards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo/src")).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Run sitroom in `cwd` with its config and data homes in the temp folder.
    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sitroom"))
            .args(args)
            .current_dir(cwd)
            // Not XDG_*: Windows ignores those and uses its Known Folders.
            .env("SITROOM_CONFIG_DIR", self.0.join("xdg/config"))
            .env("SITROOM_DURABLE_DIR", self.0.join("xdg/data"))
            .env_remove("SITROOM_ROOT")
            .env_remove("SITROOM_MACHINE_CONFIG")
            // Not a test binary: don't let tiered-dirs pick a scratch root.
            .env_remove("CARGO")
            .output()
            .unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn import_list_export_and_layers() {
    let t = Temp::new();
    let config = t.write(
        "config.yaml",
        "modules:\n  cswap: { demo: true }\nscreens:\n  - name: main\n    tiles: [{ module: cswap }]\n",
    );
    let config = config.to_str().unwrap();
    let shared = t.write(
        "On Call.yaml",
        "description: Pager rotation\nmodules:\n  pd: { package: pagerduty-missing }\n\
         screens:\n  - name: pager\n    tiles: [{ module: cswap }]\n",
    );
    t.write(
        "repo/.sitroom.yaml",
        "screens:\n  - name: repo\n    tiles: [{ module: cswap }]\n",
    );
    let repo = t.0.join("repo/src");

    let out = t.run(
        &repo,
        &["--config", config, "import", shared.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", text(&out));
    let saved = t.0.join("xdg/config/sitroom/dashboards/on-call.yaml");
    assert!(saved.is_file(), "{}", text(&out));
    assert!(
        text(&out).contains("plugin package \"pagerduty-missing\""),
        "{}",
        text(&out)
    );
    assert!(
        text(&out).contains("sitroom plugin install"),
        "{}",
        text(&out)
    );

    // A second import of the same name is refused.
    let out = t.run(
        &repo,
        &["--config", config, "import", shared.to_str().unwrap()],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("--force"), "{}", text(&out));

    let out = t.run(&repo, &["--config", config, "dashboards", "list"]);
    let listed = text(&out);
    assert!(listed.contains("on-call  Pager rotation"), "{listed}");
    assert!(listed.contains("project file:"), "{listed}");

    // Layers, in order. Sitroom needs a package that isn't installed.
    let out = t.run(
        &repo,
        &["--config", config, "--dashboard", "on-call", "config"],
    );
    let shown = text(&out);
    assert!(shown.contains("machine"), "{shown}");
    assert!(shown.contains("on-call.yaml"), "{shown}");
    assert!(shown.contains(".sitroom.yaml"), "{shown}");
    assert!(shown.contains("pagerduty-missing"), "{shown}");
    let out = t.run(&repo, &["--config", config, "--no-project", "config"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains(".sitroom.yaml"), "{}", text(&out));

    let out = t.run(&repo, &["--config", config, "export", "on-call"]);
    assert_eq!(out.stdout, std::fs::read(&shared).unwrap());
    let copy = t.0.join("copy.yaml");
    let out = t.run(
        &repo,
        &[
            "--config",
            config,
            "export",
            "on-call",
            copy.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        std::fs::read(&copy).unwrap(),
        std::fs::read(&shared).unwrap()
    );

    let out = t.run(
        &repo,
        &["--config", config, "--dashboard", "nope", "config"],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("there is: on-call"), "{}", text(&out));
}
