//! `dashboard plugin pack | install | list | update | remove`, run as the
//! binary against a local index made from the example packages. The data
//! home is a scratch folder; nothing touches the network.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "dashboard-marketplace-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn market(&self) -> PathBuf {
        self.root.join("market")
    }

    fn index(&self) -> String {
        self.market().join("index.json").display().to_string()
    }

    fn plugins(&self) -> PathBuf {
        self.root.join("data/dashboard/plugins")
    }

    /// Run the dashboard with this folder as the data home, feeding `stdin`.
    fn run(&self, args: &[&str], stdin: &str) -> Output {
        self.run_env(args, stdin, &[])
    }

    /// `run`, with these environment variables set.
    fn run_env(&self, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dashboard"));
        for (key, _) in std::env::vars() {
            if key.starts_with("DASHBOARD_") {
                cmd.env_remove(key);
            }
        }
        let mut child = cmd
            .args(args)
            .env("XDG_DATA_HOME", self.root.join("data"))
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args, "");
        assert!(out.status.success(), "{args:?}: {}", text(&out));
        String::from_utf8(out.stdout).unwrap()
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args, "");
        assert!(
            !out.status.success(),
            "{args:?} should fail: {}",
            text(&out)
        );
        text(&out)
    }

    fn pack(&self, dirs: &[&Path]) {
        self.pack_into(dirs, &self.market());
    }

    fn pack_into(&self, dirs: &[&Path], market: &Path) {
        let mut args = vec!["plugin", "pack"];
        let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        args.extend(dirs.iter().map(String::as_str));
        let out = market.display().to_string();
        args.extend(["--out", &out]);
        self.ok(&args);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn example(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins")
        .join(id)
}

#[test]
fn install_list_update_remove_from_a_local_index() {
    let s = Scratch::new("cycle");
    s.pack(&[&example("hello"), &example("latency")]);
    let index = s.index();

    // Asks first: "n" installs nothing, and so does no answer at all.
    let out = s.run(&["plugin", "install", "hello", "--index", &index], "n\n");
    assert!(out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("runs     python3 hello.py"), "{shown}");
    assert!(shown.contains("checked against the index"), "{shown}");
    assert!(shown.contains("not installed"), "{shown}");
    assert!(!s.plugins().join("hello").exists());
    let err = s.fails(&["plugin", "install", "hello", "--index", &index]);
    assert!(err.contains("--yes"), "{err}");

    // "y" installs it, where the dashboard looks for packages.
    let out = s.run(&["plugin", "install", "hello", "--index", &index], "y\n");
    assert!(out.status.success(), "{}", text(&out));
    let installed = s.plugins().join("hello");
    assert!(installed.join("dashboard-plugin.yaml").is_file());
    assert!(installed.join("hello.py").is_file());
    let found = dashboard_config::packages::Packages::load_from(&[s.plugins()]);
    assert!(found.errors.is_empty(), "{:?}", found.errors);
    assert_eq!(found.get("hello").unwrap().package.version, "0.1.0");
    let record = std::fs::read_to_string(s.plugins().join("installed.json")).unwrap();
    assert!(record.contains("index.json"), "{record}");

    let list = s.ok(&["plugin", "list", "--index", &index]);
    assert!(list.contains("hello    0.1.0     from"), "{list}");
    assert!(list.contains("available from"), "{list}");
    assert!(list.contains("latency"), "{list}");

    // A newer version in the index: list says so, and update installs it.
    let src = s.root.join("src/hello");
    std::fs::create_dir_all(&src).unwrap();
    for name in ["dashboard-plugin.yaml", "hello.py"] {
        std::fs::copy(example("hello").join(name), src.join(name)).unwrap();
    }
    let yaml = std::fs::read_to_string(src.join("dashboard-plugin.yaml")).unwrap();
    std::fs::write(
        src.join("dashboard-plugin.yaml"),
        yaml.replace("version: 0.1.0", "version: 0.2.0"),
    )
    .unwrap();
    s.pack(&[&src]);
    let list = s.ok(&["plugin", "list", "--index", &index]);
    assert!(list.contains("(update: 0.2.0)"), "{list}");
    // The index it came from is remembered, so update needs no --index.
    let out = s.ok(&["plugin", "update", "--yes"]);
    assert!(out.contains("replaces version 0.1.0"), "{out}");
    let yaml = std::fs::read_to_string(installed.join("dashboard-plugin.yaml")).unwrap();
    assert!(yaml.contains("version: 0.2.0"));
    assert!(s.ok(&["plugin", "update"]).contains("up to date"));

    s.ok(&["plugin", "remove", "hello", "--yes"]);
    assert!(!installed.exists());
    let record = std::fs::read_to_string(s.plugins().join("installed.json")).unwrap();
    assert!(!record.contains("hello"), "{record}");
    assert!(
        s.fails(&["plugin", "remove", "hello", "--yes"])
            .contains("isn't installed")
    );
    assert!(
        s.fails(&["plugin", "remove", "../x", "--yes"])
            .contains("not a package ID")
    );
}

#[test]
fn a_changed_archive_is_refused() {
    let s = Scratch::new("hash");
    s.pack(&[&example("hello")]);
    let archive = s.market().join("packages/hello-0.1.0.tar.gz");
    let mut bytes = std::fs::read(&archive).unwrap();
    bytes.push(0);
    std::fs::write(&archive, &bytes).unwrap();

    let err = s.fails(&["plugin", "install", "hello", "--index", &s.index(), "--yes"]);
    assert!(err.contains("doesn't match the index"), "{err}");
    assert!(!s.plugins().join("hello").exists());

    // A file with --sha256: checked the same way.
    let path = archive.display().to_string();
    let err = s.fails(&["plugin", "install", &path, "--sha256", "00", "--yes"]);
    assert!(err.contains("doesn't match --sha256"), "{err}");
}

#[test]
fn installs_from_a_file_or_a_folder() {
    let s = Scratch::new("paths");
    s.pack(&[&example("latency")]);
    let archive = s.market().join("packages/latency-0.1.0.tar.gz");
    let sha = etag_write::etag(&std::fs::read(&archive).unwrap());
    let path = archive.display().to_string();
    let out = s.ok(&["plugin", "install", &path, "--sha256", &sha, "--yes"]);
    assert!(out.contains("checked against --sha256"), "{out}");
    assert!(s.plugins().join("latency/dashboard-plugin.yaml").is_file());

    let folder = example("hello").display().to_string();
    let out = s.ok(&["plugin", "install", &folder, "--yes"]);
    assert!(out.contains("no hash given"), "{out}");
    assert!(s.plugins().join("hello/hello.py").is_file());
    // Installed from a path, so update doesn't know where a newer one is.
    let out = s.ok(&["plugin", "update", "hello"]);
    assert!(out.contains("not from an index"), "{out}");
}

/// A copy of the hello example at `version`, in the scratch folder.
fn hello_at(s: &Scratch, version: &str) -> PathBuf {
    let src = s.root.join(format!("src-{version}/hello"));
    std::fs::create_dir_all(&src).unwrap();
    for name in ["dashboard-plugin.yaml", "hello.py"] {
        std::fs::copy(example("hello").join(name), src.join(name)).unwrap();
    }
    let yaml = std::fs::read_to_string(src.join("dashboard-plugin.yaml")).unwrap();
    std::fs::write(
        src.join("dashboard-plugin.yaml"),
        yaml.replace("version: 0.1.0", &format!("version: {version}")),
    )
    .unwrap();
    src
}

#[test]
fn update_keeps_to_the_recorded_index() {
    let s = Scratch::new("which-index");
    s.pack(&[&example("hello")]);
    let other = s.root.join("other");
    s.pack_into(&[&hello_at(&s, "0.2.0")], &other);
    let other_index = other.join("index.json").display().to_string();
    s.ok(&["plugin", "install", "hello", "--index", &s.index(), "--yes"]);
    s.ok(&[
        "plugin",
        "install",
        &example("latency").display().to_string(),
        "--yes",
    ]);
    let env = [("DASHBOARD_PLUGIN_INDEX", other_index.as_str())];

    // The variable doesn't replace the index hello came from, and doesn't
    // reach latency, which came from a folder.
    let out = s.run_env(&["plugin", "update", "--yes"], "", &env);
    assert!(out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("hello 0.1.0: up to date"), "{shown}");
    assert!(shown.contains("latency: installed from"), "{shown}");

    // --index does.
    let out = s.ok(&[
        "plugin",
        "update",
        "hello",
        "--index",
        &other_index,
        "--yes",
    ]);
    assert!(out.contains("replaces version 0.1.0"), "{out}");

    // A folder install is updated from the variable's index only when named;
    // latency isn't in it, which is said and isn't an error.
    let out = s.run_env(&["plugin", "update", "latency", "--yes"], "", &env);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("not in the index"), "{}", text(&out));

    // hello now records the other index. With it gone, hello fails, but
    // latency (after it) is still looked at, and the command fails at the end.
    std::fs::remove_file(other.join("index.json")).unwrap();
    let out = s.run(&["plugin", "update", "--yes"], "");
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out);
    assert!(shown.contains("latency: installed from"), "{shown}");
    assert!(shown.contains("not updated: hello"), "{shown}");
}

#[test]
fn a_plain_http_index_is_refused() {
    let s = Scratch::new("http");
    let err = s.fails(&[
        "plugin",
        "install",
        "hello",
        "--index",
        "http://example.invalid/index.json",
        "--yes",
    ]);
    assert!(err.contains("not http://"), "{err}");
}

#[test]
fn refuses_what_it_cannot_check() {
    let s = Scratch::new("refuse");
    // A web URL needs a hash; this fails before any download.
    let err = s.fails(&[
        "plugin",
        "install",
        "https://example.invalid/a.tar.gz",
        "--yes",
    ]);
    assert!(err.contains("--sha256"), "{err}");
    // An ID needs an index.
    let err = s.fails(&["plugin", "install", "hello", "--yes"]);
    assert!(err.contains("--index"), "{err}");
    let err = s.fails(&["plugin", "install", "Not An Id", "--yes"]);
    assert!(err.contains("not a folder"), "{err}");
}

#[test]
fn every_ext_package_packs() {
    // scripts/build-marketplace.sh packs these; each must be a valid package
    // with no links in it.
    let s = Scratch::new("ext");
    let ext = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&ext)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("dashboard-plugin.yaml").is_file())
        .collect();
    dirs.sort();
    let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    s.pack(&refs);
    let index: dashboard_contract::Index =
        serde_json::from_slice(&std::fs::read(s.market().join("index.json")).unwrap()).unwrap();
    assert_eq!(index.plugins.len(), dirs.len());
    let err = s.fails(&[
        "plugin",
        "install",
        "clock-py",
        "--index",
        &s.index(),
        "--sha256",
        "00",
    ]);
    assert!(err.contains("the index gives the hash"), "{err}");
    s.ok(&[
        "plugin",
        "install",
        "clock-py",
        "--index",
        &s.index(),
        "--yes",
    ]);
}
