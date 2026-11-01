//! Installed plugin packages: folders holding a `dashboard-plugin.yaml` (see
//! docs/plugins.md). Looked for in two places, first match wins:
//!
//! - `plugins/<id>/` next to the config file, for packages kept with a config;
//! - `<data home>/dashboard/plugins/<id>/`, e.g. `~/.local/share/dashboard/plugins/`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use config_layers::tiered_dirs::{Convention, ResolveOptions, TieredDirs, env_vars};
use dashboard_contract::check::check_view;
use dashboard_contract::{CatalogEntry, PACKAGE_FILE, Package};

use crate::{TOOL, VENDOR};

/// A package found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub package: Package,
    /// The package's folder. Its `run:` starts here.
    pub dir: PathBuf,
}

impl Installed {
    /// The command to start it. A program given as a path is made absolute,
    /// so it doesn't depend on how the OS resolves it against the folder.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = self.package.run.clone();
        if let Some(program) = argv.first_mut()
            && Path::new(program.as_str()).components().count() > 1
        {
            *program = self.dir.join(&*program).display().to_string();
        }
        argv
    }

    pub fn entry(&self) -> CatalogEntry {
        self.package.entry(&self.dir.display().to_string())
    }
}

/// Packages that can be used with this config, and the problems with any
/// folder that isn't a valid package (to show, not to fail on).
#[derive(Debug, Default)]
pub struct Packages {
    pub installed: Vec<Installed>,
    pub errors: Vec<String>,
}

impl Packages {
    /// Packages for the config file at `config_path`: next to it, then in
    /// the data home.
    pub fn load(config_path: &Path) -> Self {
        let mut dirs = vec![];
        if let Some(dir) = config_path.parent() {
            dirs.push(dir.join("plugins"));
        }
        // Without a data home, the packages next to the config still load.
        let data = data_dir();
        if let Ok(dir) = &data {
            dirs.push(dir.clone());
        }
        let mut out = Self::load_from(&dirs);
        if let Err(err) = data {
            out.errors.push(format!(
                "finding the plugins folder in the data home: {err:#}"
            ));
        }
        out
    }

    /// Packages in these folders, each holding one folder per package. A
    /// missing folder is fine. An ID found twice keeps the first.
    pub fn load_from(dirs: &[PathBuf]) -> Self {
        let mut out = Self::default();
        for dir in dirs {
            let Ok(read) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut folders: Vec<PathBuf> = read
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.join(PACKAGE_FILE).is_file())
                .collect();
            folders.sort();
            for folder in folders {
                match load_package(&folder) {
                    Ok(p) if out.get(&p.package.id).is_some() => {}
                    Ok(p) => out.installed.push(p),
                    Err(err) => out.errors.push(format!("{err:#}")),
                }
            }
        }
        out
    }

    pub fn get(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|p| p.package.id == id)
    }
}

/// `<data home>/dashboard/plugins`, e.g. `~/.local/share/dashboard/plugins`.
/// Moved by `XDG_DATA_HOME`.
pub fn data_dir() -> Result<PathBuf> {
    let dirs = TieredDirs::from_env(
        VENDOR,
        &env_vars!("DASHBOARD"),
        ResolveOptions::new().with_convention(Convention::Xdg),
    )?;
    Ok(dirs.standalone(TOOL)?.durable().into_path().join("plugins"))
}

/// Read and check the package in `dir`. Its folder name must be its ID.
pub fn load_package(dir: &Path) -> Result<Installed> {
    let file = dir.join(PACKAGE_FILE);
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let package = parse_package(&text).with_context(|| format!("in {}", file.display()))?;
    let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if folder != package.id {
        bail!(
            "{}: id is {:?} but the folder is {folder:?}; they must match",
            file.display(),
            package.id
        );
    }
    let dir = std::fs::canonicalize(dir).with_context(|| format!("resolving {}", dir.display()))?;
    Ok(Installed { package, dir })
}

/// Parse a `dashboard-plugin.yaml` and check what serde can't.
pub fn parse_package(text: &str) -> Result<Package> {
    let p: Package = serde_saphyr::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))?;
    if !valid_id(&p.id) {
        bail!(
            "id {:?}: use lowercase letters, digits and dashes, starting with a letter",
            p.id
        );
    }
    for (field, value) in [
        ("version", &p.version),
        ("name", &p.name),
        ("description", &p.description),
    ] {
        if value.trim().is_empty() {
            bail!("{field} is empty");
        }
    }
    let Some(program) = p.run.first() else {
        bail!("run: needs at least a program name");
    };
    let path = Path::new(program);
    if path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        bail!("run: {program:?} must be a program on PATH or a path inside the package");
    }
    if let Some(runtime) = &p.runtime
        && !valid_requirement(runtime)
    {
        bail!("runtime {runtime:?}: write a program name and an optional minimum, e.g. node>=22");
    }
    if let Some(schema) = &p.config_schema
        && !schema.is_object()
    {
        bail!("config_schema must be a JSON Schema object");
    }
    let problems: Vec<String> = check_view(&p.demo)
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect();
    if !problems.is_empty() {
        bail!("demo: {}", problems.join("; "));
    }
    Ok(p)
}

fn valid_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && !id.ends_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `python3` or `python3>=3.9`.
fn valid_requirement(r: &str) -> bool {
    let (name, version) = match r.split_once(">=") {
        Some((n, v)) => (n, Some(v)),
        None => (r, None),
    };
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && version.is_none_or(|v| {
            !v.is_empty()
                && v.split('.')
                    .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = r#"
id: hello
version: 0.1.0
name: Hello
description: A greeting and a clock.
tags: [example]
run: [python3, hello.py]
runtime: python3>=3.9
size: small
config_schema:
  type: object
  properties:
    name: { type: string }
demo:
  status: ok
  summary: hello, world
  root:
    type: text
    text: "Hello, **world**"
    markdown: true
"#;

    #[test]
    fn parses_a_package() {
        let p = parse_package(HELLO).unwrap();
        assert_eq!(p.id, "hello");
        assert_eq!(p.run, ["python3", "hello.py"]);
        assert_eq!(p.size, Some(dashboard_contract::Size::Small));
        assert_eq!(p.config_schema.unwrap()["type"], "object");
    }

    #[test]
    fn rejects_bad_packages() {
        let bad = |from: &str, to: &str| {
            let text = HELLO.replacen(from, to, 1);
            assert_ne!(text, HELLO, "{from}");
            parse_package(&text).unwrap_err().to_string()
        };
        assert!(bad("id: hello", "id: Hello").contains("lowercase"));
        assert!(bad("id: hello", "id: -x").contains("lowercase"));
        assert!(bad("version: 0.1.0", "version: ''").contains("version"));
        assert!(bad("run: [python3, hello.py]", "run: []").contains("program"));
        assert!(bad("run: [python3", "run: [/usr/bin/python3").contains("inside"));
        assert!(bad("run: [python3", "run: [../x/python3").contains("inside"));
        assert!(bad("python3>=3.9", "python3 >= 3").contains("runtime"));
        assert!(bad("python3>=3.9", "python3>=3.x").contains("runtime"));
        assert!(bad("tags:", "colour: red\ntags:").contains("colour"));
        assert!(bad("type: text", "type: nope").contains("nope"));
        // A demo with a problem the schema can't see.
        let gauge = "type: gauge\n    label: x\n    percent: 140";
        let text = HELLO.replace(
            "type: text\n    text: \"Hello, **world**\"\n    markdown: true",
            gauge,
        );
        assert!(
            parse_package(&text)
                .unwrap_err()
                .to_string()
                .contains("demo")
        );
    }

    #[test]
    fn requirements() {
        assert!(valid_requirement("node"));
        assert!(valid_requirement("node>=22"));
        assert!(valid_requirement("python3>=3.9"));
        assert!(!valid_requirement(">=3"));
        assert!(!valid_requirement("node>="));
    }

    #[test]
    fn loads_installed_packages() {
        let root =
            std::env::temp_dir().join(format!("dashboard-packages-test-{}", std::process::id()));
        // A folder left by an earlier run with the same pid would change the results.
        let _ = std::fs::remove_dir_all(&root);
        let (first, second) = (root.join("a"), root.join("b"));
        let write = |dir: &Path, folder: &str, text: &str| {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
            std::fs::write(dir.join(folder).join(PACKAGE_FILE), text).unwrap();
        };
        write(&first, "hello", HELLO);
        write(
            &first,
            "bin-tool",
            &HELLO
                .replace("id: hello", "id: bin-tool")
                .replace("[python3, hello.py]", "[bin/tool, --fast]"),
        );
        // Wrong folder name, and a broken file: reported, not fatal.
        write(&first, "other", HELLO);
        write(&first, "broken", "id: [\n");
        // A folder with no package file is ignored.
        std::fs::create_dir_all(first.join("notes")).unwrap();
        // The same ID later on is shadowed by the first.
        write(&second, "hello", &HELLO.replace("Hello", "Second"));

        let found = Packages::load_from(&[first.clone(), second, root.join("missing")]);
        let ids: Vec<&str> = found
            .installed
            .iter()
            .map(|p| p.package.id.as_str())
            .collect();
        assert_eq!(ids, ["bin-tool", "hello"]);
        assert_eq!(found.get("hello").unwrap().package.name, "Hello");
        assert_eq!(found.errors.len(), 2, "{:?}", found.errors);
        assert!(found.errors.iter().any(|e| e.contains("must match")));

        // Paths in `run` resolve inside the folder; programs on PATH stay as they are.
        let tool = found.get("bin-tool").unwrap();
        assert!(
            tool.argv()[0].ends_with("/bin-tool/bin/tool"),
            "{:?}",
            tool.argv()
        );
        assert!(Path::new(&tool.argv()[0]).is_absolute());
        assert_eq!(tool.argv()[1], "--fast");
        assert_eq!(found.get("hello").unwrap().argv()[0], "python3");

        let entry = found.get("hello").unwrap().entry();
        assert_eq!(entry.kind, dashboard_contract::EntryKind::Plugin);
        assert_eq!(entry.requirements.binaries, ["python3>=3.9"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn example_packages_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
        let found = Packages::load_from(std::slice::from_ref(&dir));
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        // The four examples are folders here. Everything else is a link to a
        // token-API package in plugins/ext, so examples/try.yaml can show its demo.
        let examples = ["gh-actions", "git-repos", "hello", "latency"];
        for id in examples {
            assert!(found.get(id).is_some(), "{id}");
        }
        for p in &found.installed {
            let id = p.package.id.as_str();
            if examples.contains(&id) {
                continue;
            }
            let link = std::fs::read_link(dir.join(id))
                .unwrap_or_else(|_| panic!("{id}: not an example, so it should be a link"));
            assert_eq!(link, Path::new("../../plugins/ext").join(id), "{id}");
        }
        // examples/plugins.yaml finds them next to itself.
        let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins.yaml");
        let found = Packages::load(&config);
        assert!(found.get("latency").is_some());
    }

    #[test]
    fn clock_packages_match_native_demo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = Packages::load_from(&[root.join("plugins/ext")]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        // The clock plugins' demo is the native module's view of the same sample.
        let text =
            std::fs::read_to_string(root.join("crates/modules-fun/testdata/clock/demo.view.json"))
                .unwrap();
        let expected: dashboard_contract::View = serde_json::from_str(&text).unwrap();
        for id in ["clock-ts", "clock-py"] {
            let p = found.get(id).unwrap_or_else(|| panic!("{id} not found"));
            assert_eq!(p.package.demo, expected, "{id}");
        }
    }

    #[test]
    fn textfile_packages_match_native_demo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = Packages::load_from(&[root.join("plugins/ext")]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        // The textfile plugins' demo is the native module's view of the same sample.
        let text = std::fs::read_to_string(
            root.join("crates/modules-system/testdata/textfile/notes.md.view.json"),
        )
        .unwrap();
        let expected: dashboard_contract::View = serde_json::from_str(&text).unwrap();
        for id in ["textfile-ts", "textfile-py"] {
            let p = found.get(id).unwrap_or_else(|| panic!("{id} not found"));
            assert_eq!(p.package.demo, expected, "{id}");
        }
    }

    #[test]
    fn battery_packages_match_native_demo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = Packages::load_from(&[root.join("plugins/ext")]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        // The battery plugins' demo is the native module's view of the same sample.
        let text = std::fs::read_to_string(
            root.join("crates/modules-system/testdata/battery/pmset-discharging.view.json"),
        )
        .unwrap();
        let expected: dashboard_contract::View = serde_json::from_str(&text).unwrap();
        for id in ["battery-ts", "battery-py"] {
            let p = found.get(id).unwrap_or_else(|| panic!("{id} not found"));
            assert_eq!(p.package.demo, expected, "{id}");
        }
    }

    #[test]
    fn feedreader_packages_match_native_demo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = Packages::load_from(&[root.join("plugins/ext")]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        // The feed reader plugins' demo is the native module's view of the same sample.
        let text = std::fs::read_to_string(
            root.join("crates/modules-fun/testdata/feedreader/demo.view.json"),
        )
        .unwrap();
        let expected: dashboard_contract::View = serde_json::from_str(&text).unwrap();
        for id in ["feedreader-ts", "feedreader-py"] {
            let p = found.get(id).unwrap_or_else(|| panic!("{id} not found"));
            assert_eq!(p.package.demo, expected, "{id}");
        }
    }

    #[test]
    fn hackernews_packages_match_native_demo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let found = Packages::load_from(&[root.join("plugins/ext")]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        // The hackernews plugins' demo is the native module's demo view.
        let text = std::fs::read_to_string(
            root.join("crates/modules-fun/testdata/hackernews/demo.view.json"),
        )
        .unwrap();
        let expected: dashboard_contract::View = serde_json::from_str(&text).unwrap();
        for id in ["hackernews-ts", "hackernews-py"] {
            let p = found.get(id).unwrap_or_else(|| panic!("{id} not found"));
            assert_eq!(p.package.demo, expected, "{id}");
        }
    }

    #[test]
    fn ext_packages_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext");
        let found = Packages::load_from(&[dir]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        for id in ["docker-py", "docker-ts"] {
            assert!(found.get(id).is_some(), "{id}");
        }
    }

    #[test]
    fn ext_weather_packages_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/ext");
        let found = Packages::load_from(&[dir]);
        assert!(found.errors.is_empty(), "{:?}", found.errors);
        for id in ["weather-py", "weather-ts"] {
            assert!(found.get(id).is_some(), "{id}");
        }
    }
}
