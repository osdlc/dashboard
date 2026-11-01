//! The config is read from up to three files, merged in this order (later
//! wins):
//!
//! 1. the machine file, `config.yaml` (or `--config`);
//! 2. a saved dashboard, when one is chosen (`--dashboard work`);
//! 3. the project file, `.dashboard.yaml`, found by walking up from the
//!    working directory. Only used once trusted (see `trust`): it can start
//!    programs.
//!
//! How each key merges is in docs/dashboards.md: `modules:` merges per
//! module, a dashboard's `screens:` replace the ones below it, and the
//! project file's screens are added in front.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::dashboards::Registry;
use crate::trust::Trust;
use crate::{Config, DEFAULT_CONFIG, parse};

/// The project file's name.
pub const PROJECT_FILE: &str = ".dashboard.yaml";

/// Which files to read.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// The machine file. `None`: the default path.
    pub machine: Option<PathBuf>,
    /// A saved dashboard, by name.
    pub dashboard: Option<String>,
    /// Look for a project file from here upward. `None`: no project layer.
    pub project_from: Option<PathBuf>,
    /// Where saved dashboards are. `None`: the user's config home.
    pub registry: Option<PathBuf>,
    /// The list of trusted project files. `None`: the one in the data home.
    pub trust: Option<PathBuf>,
}

impl Sources {
    pub fn trust(&self) -> Result<Trust> {
        match &self.trust {
            Some(path) => Ok(Trust::new(path)),
            None => Trust::open_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Machine,
    Dashboard,
    Project,
}

impl LayerKind {
    pub fn label(self) -> &'static str {
        match self {
            LayerKind::Machine => "machine",
            LayerKind::Dashboard => "dashboard",
            LayerKind::Project => "project",
        }
    }
}

/// A file that was read.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub kind: LayerKind,
    pub path: PathBuf,
}

/// The merged config and where it came from.
#[derive(Debug, Clone)]
pub struct Layered {
    pub config: Config,
    /// The machine file's path, whether or not it exists.
    pub machine: PathBuf,
    /// False when no file at all was found and `DEFAULT_CONFIG` is in use.
    pub exists: bool,
    /// The files read, lowest precedence first.
    pub layers: Vec<Layer>,
    /// The saved dashboard in use.
    pub dashboard: Option<String>,
    /// For each module, the folder of the file that set it last: a plugin's
    /// relative paths start there.
    pub module_dirs: BTreeMap<String, PathBuf>,
    /// Names of the screens the project file added.
    pub project_screens: BTreeSet<String>,
    /// Screens below the project file that one of its screens replaced, with
    /// their place in that list. Saving puts them back (see `restore_shadowed`).
    pub shadowed: Vec<(usize, Value)>,
    /// A project file that was found but not used, because it isn't trusted.
    pub untrusted: Option<PathBuf>,
    /// The etag (see `etag_write`) of the bytes read from `write_path()`,
    /// `None` when that file doesn't exist. A write based on another version
    /// is refused.
    pub etag: Option<String>,
}

impl Layered {
    /// The file settings changed from a renderer are written to: the saved
    /// dashboard in use, else the machine file. Never the project file,
    /// which is usually checked in.
    pub fn write_path(&self) -> &Path {
        self.layer(LayerKind::Dashboard).unwrap_or(&self.machine)
    }

    pub fn layer(&self, kind: LayerKind) -> Option<&Path> {
        self.layers
            .iter()
            .find(|l| l.kind == kind)
            .map(|l| l.path.as_path())
    }

    /// Every file read, for watching.
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.layers.iter().map(|l| l.path.clone()).collect();
        if !paths.contains(&self.machine) {
            paths.push(self.machine.clone());
        }
        paths
    }

    /// Put back, in their old places, the screens the project file replaced.
    /// `screens` are the ones to write, without the project's.
    pub fn restore_shadowed(&self, screens: &mut Vec<Value>) {
        for (i, screen) in &self.shadowed {
            screens.insert((*i).min(screens.len()), screen.clone());
        }
    }

    /// Why the project file isn't used, if one was skipped.
    pub fn untrusted_notice(&self) -> Option<String> {
        self.untrusted.as_ref().map(|p| {
            format!(
                "{} is not trusted, so it is not used: it can run programs. \
                 Read it, then run `dashboard trust` in that folder to use it.",
                p.display()
            )
        })
    }
}

/// Find the project file from `dir` upward.
pub fn find_project(dir: &Path) -> Option<PathBuf> {
    config_layers::file::find_up(dir, PROJECT_FILE)
}

/// Read and merge the layers. Each file must be a valid config on its own;
/// the error names the file.
pub fn load(sources: &Sources) -> Result<Layered> {
    let machine = match &sources.machine {
        Some(p) => p.clone(),
        None => crate::default_path()?,
    };
    let mut files = vec![];
    if machine.exists() {
        files.push(Layer {
            kind: LayerKind::Machine,
            path: machine.clone(),
        });
    }
    if let Some(name) = &sources.dashboard {
        let registry = match &sources.registry {
            Some(dir) => Registry::new(dir),
            None => Registry::open_default()?,
        };
        files.push(Layer {
            kind: LayerKind::Dashboard,
            path: registry.find(name)?,
        });
    }
    let mut untrusted = None;
    // Read once: the text that was checked is the text that is used.
    let mut project_text = None;
    if let Some(path) = sources.project_from.as_deref().and_then(find_project) {
        let bytes = std::fs::read(&path).ok();
        let trusted = bytes
            .as_deref()
            .is_some_and(|b| sources.trust().is_ok_and(|t| t.allows_bytes(&path, b)));
        if trusted {
            // Not UTF-8: left to `read_layer`, which reports it.
            project_text = bytes.and_then(|b| String::from_utf8(b).ok());
            files.push(Layer {
                kind: LayerKind::Project,
                path,
            });
        } else {
            untrusted = Some(path);
        }
    }

    // Nothing at all: the demo, so a first run shows something.
    if files.is_empty() {
        let config = parse(DEFAULT_CONFIG).context("parsing the built-in default config")?;
        let dir = machine.parent().unwrap_or(Path::new("")).to_path_buf();
        return Ok(Layered {
            module_dirs: config
                .modules
                .keys()
                .map(|id| (id.clone(), dir.clone()))
                .collect(),
            config,
            machine,
            exists: false,
            layers: vec![],
            dashboard: None,
            project_screens: BTreeSet::new(),
            shadowed: vec![],
            untrusted,
            etag: None,
        });
    }

    let mut merged = Map::new();
    let mut module_dirs = BTreeMap::new();
    let mut project_screens = BTreeSet::new();
    let mut shadowed = vec![];
    let write_kind = if files.iter().any(|l| l.kind == LayerKind::Dashboard) {
        LayerKind::Dashboard
    } else {
        LayerKind::Machine
    };
    let mut etag = None;
    for layer in &files {
        let map = match (&layer.kind, &project_text) {
            (LayerKind::Project, Some(text)) => parse_layer(&layer.path, text)?,
            _ => {
                let text = std::fs::read_to_string(&layer.path)
                    .with_context(|| format!("reading {}", layer.path.display()))?;
                if layer.kind == write_kind {
                    etag = Some(etag_write::etag(text.as_bytes()));
                }
                parse_layer(&layer.path, &text)?
            }
        };
        let dir = layer.path.parent().unwrap_or(Path::new("")).to_path_buf();
        if let Some(Value::Object(modules)) = map.get("modules") {
            for id in modules.keys() {
                module_dirs.insert(id.clone(), dir.clone());
            }
        }
        if layer.kind == LayerKind::Project
            && let Some(Value::Array(screens)) = map.get("screens")
        {
            project_screens.extend(screens.iter().filter_map(screen_name).map(String::from));
        }
        shadowed.extend(merge(&mut merged, map, layer.kind));
    }
    let config: Config = serde_json::from_value(Value::Object(merged)).context(format!(
        "merging {}",
        files
            .iter()
            .map(|l| l.path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    // A module only named on a screen (a built-in or bundled plugin) runs
    // from the machine file's folder.
    let machine_dir = machine.parent().unwrap_or(Path::new("")).to_path_buf();
    for screen in &config.screens {
        for tile in &screen.tiles {
            module_dirs
                .entry(tile.module.clone())
                .or_insert_with(|| machine_dir.clone());
        }
    }
    Ok(Layered {
        config,
        machine,
        exists: true,
        dashboard: sources.dashboard.clone(),
        layers: files,
        module_dirs,
        project_screens,
        shadowed,
        untrusted,
        etag,
    })
}

/// A layer's text, checked first as a whole config, for clear errors, then
/// read as a map of its top-level keys to merge. An empty or comment-only
/// file is an empty map.
fn parse_layer(path: &Path, text: &str) -> Result<Map<String, Value>> {
    parse(text).with_context(|| format!("in {}", path.display()))?;
    let value: Option<Value> =
        serde_saphyr::from_str(text).map_err(|e| anyhow::anyhow!("{e} in {}", path.display()))?;
    match value {
        Some(Value::Object(map)) => Ok(map),
        _ => Ok(Map::new()),
    }
}

/// Lay `top` over `base`. `modules:` merges per module ID (a module's
/// settings come whole from the last file that sets it). `screens:` from a
/// project file go in front, replacing any screen of the same name; from any
/// other file they replace the screens below. Every other key: the later
/// file wins. An empty `modules:` or `screens:` changes nothing. Returns the
/// screens a project file replaced, with their places.
fn merge(
    base: &mut Map<String, Value>,
    top: Map<String, Value>,
    kind: LayerKind,
) -> Vec<(usize, Value)> {
    let mut shadowed = vec![];
    for (key, value) in top {
        match (key.as_str(), value) {
            // An empty `modules:` or `screens:` (YAML null) parses as none on
            // its own; here too it adds nothing and keeps what is below.
            ("modules" | "screens", Value::Null) => {}
            ("modules", Value::Object(modules)) => {
                let entry = base
                    .entry("modules")
                    .or_insert_with(|| Value::Object(Map::new()));
                if !entry.is_object() {
                    *entry = Value::Object(Map::new());
                }
                if let Value::Object(into) = entry {
                    into.extend(modules);
                }
            }
            ("screens", Value::Array(mut screens)) if kind == LayerKind::Project => {
                let names: BTreeSet<String> = screens
                    .iter()
                    .filter_map(screen_name)
                    .map(String::from)
                    .collect();
                if let Some(Value::Array(below)) = base.remove("screens") {
                    for (i, s) in below.into_iter().enumerate() {
                        if screen_name(&s).is_some_and(|n| names.contains(n)) {
                            shadowed.push((i, s));
                        } else {
                            screens.push(s);
                        }
                    }
                }
                base.insert(key, Value::Array(screens));
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
    shadowed
}

fn screen_name(screen: &Value) -> Option<&str> {
    screen.get("name")?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::Theme;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dashboard-layers-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, text: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }

        /// The trust list for this test.
        fn trust_file(&self) -> PathBuf {
            self.0.join("data/trusted-projects")
        }

        /// Write a project file and trust it.
        fn trusted(&self, rel: &str, text: &str) -> PathBuf {
            let path = self.write(rel, text);
            Trust::new(self.trust_file()).allow(&path).unwrap();
            path
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const MACHINE: &str = "\
theme: dark
modules:
  cpu: { refresh: 5s }
  clock: {}
screens:
  - name: main
    tiles: [{ module: cpu }, { module: clock }]
  - name: system
    tiles: [{ module: cpu }]
";

    fn names(c: &Config) -> Vec<&str> {
        c.screens.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn machine_file_alone() {
        let t = Temp::new("machine");
        let machine = t.write("config.yaml", MACHINE);
        let l = load(&Sources {
            machine: Some(machine.clone()),
            ..Default::default()
        })
        .unwrap();
        assert!(l.exists);
        assert_eq!(l.layers.len(), 1);
        assert_eq!(l.write_path(), machine);
        assert_eq!(l.config, parse(MACHINE).unwrap());
    }

    #[test]
    fn nothing_found_is_the_demo() {
        let t = Temp::new("demo");
        let l = load(&Sources {
            machine: Some(t.0.join("config.yaml")),
            project_from: Some(t.0.clone()),
            registry: Some(t.0.join("dashboards")),
            ..Default::default()
        })
        .unwrap();
        assert!(!l.exists);
        assert_eq!(l.config, parse(DEFAULT_CONFIG).unwrap());
    }

    #[test]
    fn dashboard_replaces_screens_and_merges_modules() {
        let t = Temp::new("dashboard");
        let machine = t.write("config.yaml", MACHINE);
        let work = t.write(
            "dashboards/work.yaml",
            "description: Work\nmodules:\n  cpu: { refresh: 10s }\n  github-prs: {}\n\
             screens:\n  - name: review\n    tiles: [{ module: github-prs }]\n",
        );
        let l = load(&Sources {
            machine: Some(machine),
            dashboard: Some("work".into()),
            registry: Some(t.0.join("dashboards")),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(l.dashboard.as_deref(), Some("work"));
        assert_eq!(l.write_path(), work);
        // Kept from the machine file.
        assert_eq!(l.config.theme, Theme::Dark);
        assert!(l.config.modules.contains_key("clock"));
        // The dashboard's module settings win, whole.
        assert_eq!(
            l.config.modules["cpu"].refresh,
            Some(std::time::Duration::from_secs(10))
        );
        assert_eq!(names(&l.config), ["review"]);
        assert_eq!(l.config.description.as_deref(), Some("Work"));

        // An unknown dashboard is an error that names the folder's dashboards.
        let err = load(&Sources {
            dashboard: Some("play".into()),
            registry: Some(t.0.join("dashboards")),
            machine: Some(t.0.join("config.yaml")),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains("work"), "{err}");
    }

    #[test]
    fn project_file_adds_screens_in_front() {
        let t = Temp::new("project");
        let machine = t.write("home/config.yaml", MACHINE);
        let project = t.trusted(
            "repo/.dashboard.yaml",
            "modules:\n  ci:\n    plugin: [python3, ci.py]\n\
             screens:\n  - name: repo\n    tiles: [{ module: ci }]\n  - name: system\n    tiles: [{ module: clock }]\n",
        );
        let nested = t.0.join("repo/src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        let l = load(&Sources {
            machine: Some(machine.clone()),
            project_from: Some(nested),
            trust: Some(t.trust_file()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(l.layer(LayerKind::Project), Some(project.as_path()));
        // Repo screens first; its "system" replaces the machine file's.
        assert_eq!(names(&l.config), ["repo", "system", "main"]);
        assert_eq!(l.config.screens[1].tiles[0].module, "clock");
        assert_eq!(
            l.project_screens,
            BTreeSet::from(["repo".to_string(), "system".to_string()])
        );
        // The project's plugin runs from the repo; the rest from the machine file's folder.
        assert_eq!(l.module_dirs["ci"], t.0.join("repo"));
        assert_eq!(l.module_dirs["cpu"], t.0.join("home"));
        // Writes never go to the project file.
        assert_eq!(l.write_path(), machine);
        assert_eq!(l.paths().len(), 2);
        // The machine file's "system", hidden by the project's, is kept for saving.
        assert_eq!(l.shadowed.len(), 1);
        assert_eq!(l.shadowed[0].0, 1);
        assert_eq!(screen_name(&l.shadowed[0].1), Some("system"));
        let mut own = vec![serde_json::json!({ "name": "main", "tiles": [] })];
        l.restore_shadowed(&mut own);
        let own: Vec<_> = own.iter().filter_map(screen_name).collect();
        assert_eq!(own, ["main", "system"]);
    }

    /// `modules:` or `screens:` with nothing under it loads as it parses on
    /// its own, and keeps the layer below's.
    #[test]
    fn an_empty_key_changes_nothing() {
        let t = Temp::new("empty-key");
        let machine = t.write("home/config.yaml", "modules:\ntheme: dark\n");
        let l = load(&Sources {
            machine: Some(machine.clone()),
            ..Default::default()
        })
        .unwrap();
        assert!(l.config.modules.is_empty());

        std::fs::write(&machine, MACHINE).unwrap();
        t.trusted("repo/.dashboard.yaml", "modules:\nscreens:\n");
        let l = load(&Sources {
            machine: Some(machine),
            project_from: Some(t.0.join("repo")),
            trust: Some(t.trust_file()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(l.layers.len(), 2);
        assert_eq!(l.config.modules, parse(MACHINE).unwrap().modules);
        assert_eq!(l.config.screens, parse(MACHINE).unwrap().screens);
    }

    #[test]
    fn an_untrusted_project_file_is_not_used() {
        let t = Temp::new("untrusted");
        let machine = t.write("config.yaml", MACHINE);
        let project = t.write(
            "repo/.dashboard.yaml",
            "modules:\n  ci:\n    plugin: [sh, -c, 'echo hi']\n",
        );
        let sources = Sources {
            machine: Some(machine),
            project_from: Some(t.0.join("repo")),
            trust: Some(t.trust_file()),
            ..Default::default()
        };
        let l = load(&sources).unwrap();
        assert_eq!(l.layer(LayerKind::Project), None);
        assert!(!l.config.modules.contains_key("ci"));
        assert_eq!(l.untrusted.as_deref(), Some(project.as_path()));
        assert!(l.untrusted_notice().unwrap().contains("dashboard trust"));

        Trust::new(t.trust_file()).allow(&project).unwrap();
        let l = load(&sources).unwrap();
        assert!(l.config.modules.contains_key("ci"));
        assert_eq!(l.untrusted, None);

        // Changed since it was trusted: not used again until trusted again.
        t.write(
            "repo/.dashboard.yaml",
            "modules:\n  ci:\n    plugin: [rm, x]\n",
        );
        assert!(load(&sources).unwrap().untrusted.is_some());
    }

    #[test]
    fn a_bad_layer_names_its_file() {
        let t = Temp::new("bad");
        let machine = t.write("config.yaml", MACHINE);
        t.trusted("repo/.dashboard.yaml", "themes: dark\n");
        let err = load(&Sources {
            machine: Some(machine),
            project_from: Some(t.0.join("repo")),
            trust: Some(t.trust_file()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(format!("{err:#}").contains(".dashboard.yaml"), "{err:#}");
    }

    #[test]
    fn empty_project_file_changes_nothing() {
        let t = Temp::new("empty");
        let machine = t.write("config.yaml", MACHINE);
        t.trusted("repo/.dashboard.yaml", "# nothing yet\n");
        let l = load(&Sources {
            machine: Some(machine),
            project_from: Some(t.0.join("repo")),
            trust: Some(t.trust_file()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(l.config, parse(MACHINE).unwrap());
    }
}
