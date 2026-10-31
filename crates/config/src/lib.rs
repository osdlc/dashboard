//! The user's config file: `<config home>/dashboard/config.yaml`.
//!
//! Read with serde-saphyr. Top-level keys are written back with yaml-splice,
//! so everything here that the settings screen can change in place is a
//! top-level key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use config_layers::machine::MachineConfigFile;
use config_layers::tiered_dirs::{Convention, ResolveOptions, TieredDirs, env_vars};
use dashboard_contract::theme::{self, ColorTheme};
use dashboard_contract::{LayoutHints, RotateSettings, ScreenLayout, Size, Theme, TilePlacement};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod add;
pub mod dashboards;
pub mod layers;
pub mod packages;
pub mod trust;

pub use add::{NewModule, add_module};

pub const TOOL: &str = "dashboard";
const VENDOR: &str = "dev.sksizer";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// What this dashboard is for, in a few words. Shown by `dashboard
    /// dashboards list` and in the web view's dashboard switcher.
    pub description: Option<String>,
    pub theme: Theme,
    /// The colors: a built-in theme (`default`, `contrast`, `paper`, `ember`)
    /// or one from `color_themes:`. Default: `default`.
    pub color_theme: Option<String>,
    /// Your own color themes, by name: a built-in with some tokens changed.
    pub color_themes: BTreeMap<String, CustomTheme>,
    /// Per-module settings, keyed by tile ID. A built-in module runs when it
    /// is listed here or placed on a screen; plugins must be listed here.
    pub modules: BTreeMap<String, ModuleConfig>,
    /// If empty, one screen shows every enabled module, auto-placed.
    pub screens: Vec<Screen>,
    pub rotate: Option<Rotate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ModuleConfig {
    pub enabled: bool,
    /// Run this module as a stdio plugin: program and arguments.
    pub plugin: Option<Vec<String>>,
    /// Run this built-in module under this tile ID, so one module can show in
    /// several tiles with different settings, e.g. `builtin: demo`.
    pub builtin: Option<String>,
    /// Run this installed plugin package (see docs/plugins.md), by its ID.
    pub package: Option<String>,
    /// Override the module's suggested refresh period, e.g. `30s`, `2m`.
    /// Cannot go below the module's minimum.
    #[serde(with = "humantime_serde")]
    #[schemars(with = "Option<String>")]
    pub refresh: Option<Duration>,
    /// Override the module's suggested size.
    pub size: Option<Size>,
    /// Show the module's demo view (made-up data from its catalog entry)
    /// instead of running it: no process, no network, no token.
    pub demo: bool,
    /// Passed to the module as-is.
    pub config: serde_json::Value,
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            plugin: None,
            builtin: None,
            package: None,
            refresh: None,
            size: None,
            demo: false,
            config: serde_json::Value::Null,
        }
    }
}

/// A color theme in the config: `base` with the tokens given replaced, per
/// variant. Values are hex colors, e.g. `accent: "#c2410c"`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CustomTheme {
    /// The built-in theme to start from. Default: `default`.
    pub base: Option<String>,
    /// Tokens for the light variant, by name.
    pub light: BTreeMap<String, String>,
    /// Tokens for the dark variant, by name.
    pub dark: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub name: String,
    pub tiles: Vec<Tile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    pub module: String,
    #[serde(default)]
    pub size: Option<Size>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rotate {
    #[serde(with = "humantime_serde")]
    #[schemars(with = "String")]
    pub interval: Duration,
    /// Screens to rotate through, by name. Default: all.
    #[serde(default)]
    pub screens: Option<Vec<String>>,
    /// Idle time after user input before rotation resumes.
    #[serde(default = "default_resume_after", with = "humantime_serde")]
    #[schemars(with = "String")]
    pub resume_after: Duration,
}

fn default_resume_after() -> Duration {
    Duration::from_secs(60)
}

/// Where the config was read from, for messages and the settings screen.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    pub path: PathBuf,
    /// False when no file exists and `DEFAULT_CONFIG` is in use.
    pub exists: bool,
    /// The etag of the bytes read (see `etag_write`), `None` when there is no
    /// file. A renderer sends it back with a change, so a write based on an
    /// older version of the file is refused.
    pub etag: Option<String>,
}

/// Used when there is no config file: the demo, so a first run shows what
/// the dashboard can do. Saving a setting from the UI writes this out first.
pub const DEFAULT_CONFIG: &str = include_str!("../../../examples/demo.yaml");

/// Default config path: `<config home>/dashboard/config.yaml`, XDG layout on
/// every OS (`~/.config/dashboard/config.yaml` on macOS too). Moved by
/// `DASHBOARD_MACHINE_CONFIG`.
pub fn default_path() -> Result<PathBuf> {
    Ok(MachineConfigFile::<Config>::new(TOOL, &tiered_dirs()?)?.path())
}

/// The OS homes, XDG layout on every OS, moved by `XDG_*` or `DASHBOARD_*_DIR`.
fn tiered_dirs() -> Result<TieredDirs> {
    Ok(TieredDirs::from_env(
        VENDOR,
        &env_vars!("DASHBOARD"),
        ResolveOptions::new().with_convention(Convention::Xdg),
    )?)
}

/// Where a background `serve` keeps its record (the runtime dir: pidfiles,
/// locks) and its log (the logs dir), e.g. `~/.local/state/dashboard/...`.
pub fn run_dirs() -> Result<(PathBuf, PathBuf)> {
    let dirs = tiered_dirs()?;
    let tool = dirs.standalone(TOOL)?;
    Ok((tool.runtime().into_path(), tool.logs().into_path()))
}

/// Load from `path`, or the default path. A missing file means `DEFAULT_CONFIG`.
pub fn load(path: Option<&Path>) -> Result<Loaded> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => default_path()?,
    };
    if !path.exists() {
        return Ok(Loaded {
            config: parse(DEFAULT_CONFIG).context("parsing the built-in default config")?,
            path,
            exists: false,
            etag: None,
        });
    }
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let text = std::str::from_utf8(&bytes)
        .with_context(|| format!("reading {}: not UTF-8", path.display()))?;
    let config = parse(text).with_context(|| format!("in {}", path.display()))?;
    Ok(Loaded {
        config,
        path,
        exists: true,
        etag: Some(etag_write::etag(&bytes)),
    })
}

pub fn parse(text: &str) -> Result<Config> {
    if text.trim().is_empty() {
        return Ok(Config::default());
    }
    let config: Config = serde_saphyr::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(config)
}

impl Config {
    pub fn module(&self, id: &str) -> ModuleConfig {
        self.modules.get(id).cloned().unwrap_or_default()
    }

    /// Screens to show, given the running modules (tile ID and the module's
    /// layout hints, in spawn order). Tiles of modules that aren't running are
    /// dropped. With no `screens:` configured, one "main" screen shows every
    /// running module, highest priority first.
    pub fn screen_layouts(&self, running: &[(String, LayoutHints)]) -> Vec<ScreenLayout> {
        let hints = |id: &str| running.iter().find(|(t, _)| t == id).map(|(_, h)| h);
        let place = |id: &str, size: Option<Size>| {
            hints(id).map(|h| TilePlacement {
                tile_id: id.to_string(),
                size: size.or(self.module(id).size).unwrap_or(h.size),
            })
        };

        if self.screens.is_empty() {
            let mut ordered: Vec<&(String, LayoutHints)> = running.iter().collect();
            ordered.sort_by_key(|(_, h)| std::cmp::Reverse(h.priority));
            let tiles = ordered
                .iter()
                .filter_map(|(id, _)| place(id, None))
                .collect();
            return vec![ScreenLayout {
                name: "main".into(),
                tiles,
            }];
        }
        self.screens
            .iter()
            .map(|s| ScreenLayout {
                name: s.name.clone(),
                tiles: s
                    .tiles
                    .iter()
                    .filter_map(|t| place(&t.module, t.size))
                    .collect(),
            })
            .collect()
    }

    /// Rotation for renderers, given the screens actually shown. `None` when
    /// not configured or when there is fewer than two screens to rotate through.
    pub fn rotate_settings(&self, screens: &[ScreenLayout]) -> Option<RotateSettings> {
        let r = self.rotate.as_ref()?;
        let names: Vec<String> = match &r.screens {
            Some(names) => names.clone(),
            None => screens.iter().map(|s| s.name.clone()).collect(),
        };
        (names.len() >= 2).then_some(RotateSettings {
            interval_ms: r.interval.as_millis() as u64,
            screens: names,
            resume_after_ms: r.resume_after.as_millis() as u64,
        })
    }

    /// The color theme `color_theme:` names, with its tokens resolved.
    pub fn color_theme(&self) -> Result<ColorTheme> {
        self.color_theme_named(self.color_theme.as_deref().unwrap_or(theme::DEFAULT))
    }

    /// A built-in or configured color theme by name.
    pub fn color_theme_named(&self, name: &str) -> Result<ColorTheme> {
        if let Some(t) = theme::builtin(name) {
            return Ok(t);
        }
        let Some(custom) = self.color_themes.get(name) else {
            bail!(
                "color_theme: unknown theme {name:?} (available: {})",
                self.color_theme_names().join(", ")
            );
        };
        let base_name = custom.base.as_deref().unwrap_or(theme::DEFAULT);
        let base = theme::builtin(base_name).with_context(|| {
            format!(
                "color_themes.{name}.base: {base_name:?} is not a built-in theme ({})",
                theme::BUILTIN.join(", ")
            )
        })?;
        base.with_overrides(name, pairs(&custom.light), pairs(&custom.dark))
            .map_err(|e| anyhow::anyhow!("color_themes.{name}.{e}"))
    }

    /// Color themes to choose from: built-ins, then the config's own.
    pub fn color_theme_names(&self) -> Vec<String> {
        theme::BUILTIN
            .iter()
            .map(|s| s.to_string())
            .chain(self.color_themes.keys().cloned())
            .collect()
    }

    /// Check references between sections. `available` is every module ID the
    /// host can run (built-ins plus configured plugins). The error says which
    /// part of the config is wrong, so the caller can name the file it came
    /// from (see `layers::Layered::file_of`).
    pub fn validate(&self, available: &[String]) -> Result<(), Invalid> {
        let module = |id: &str, message: String| Invalid {
            part: Part::Module(id.to_string()),
            message,
        };
        for (id, m) in &self.modules {
            if m.plugin.as_ref().is_some_and(|argv| argv.is_empty()) {
                return Err(module(
                    id,
                    format!("modules.{id}.plugin: needs at least a program name"),
                ));
            }
            let ways = [m.plugin.is_some(), m.builtin.is_some(), m.package.is_some()];
            if ways.iter().filter(|w| **w).count() > 1 {
                return Err(module(
                    id,
                    format!("modules.{id}: set one of `plugin`, `builtin` or `package`"),
                ));
            }
        }
        let screen = |name: &str, message: String| Invalid {
            part: Part::Screen(name.to_string()),
            message,
        };
        let mut names = std::collections::BTreeSet::new();
        for s in &self.screens {
            if !names.insert(s.name.as_str()) {
                return Err(screen(
                    &s.name,
                    format!("screens: name {:?} is used twice", s.name),
                ));
            }
            for tile in &s.tiles {
                if !available.contains(&tile.module) {
                    return Err(screen(
                        &s.name,
                        format!(
                            "screen {:?}: unknown module {:?} (available: {}; it may be left out of this build, see docs/modules.md)",
                            s.name,
                            tile.module,
                            available.join(", ")
                        ),
                    ));
                }
            }
        }
        let key = |key: &str, message: String| Invalid {
            part: Part::Key(key.to_string()),
            message,
        };
        for name in self.color_themes.keys() {
            if theme::builtin(name).is_some() {
                return Err(key(
                    "color_themes",
                    format!("color_themes.{name}: a built-in theme has this name; pick another"),
                ));
            }
            self.color_theme_named(name)
                .map_err(|e| key("color_themes", e.to_string()))?;
        }
        self.color_theme()
            .map_err(|e| key("color_theme", e.to_string()))?;
        if let Some(rotate) = &self.rotate {
            let rotate_err = |message: String| Invalid {
                part: Part::Rotate,
                message,
            };
            if rotate.interval.is_zero() {
                return Err(rotate_err("rotate.interval must be more than zero".into()));
            }
            for name in rotate.screens.iter().flatten() {
                if !names.contains(name.as_str()) {
                    return Err(rotate_err(format!(
                        "rotate.screens: unknown screen {name:?}"
                    )));
                }
            }
        }
        Ok(())
    }
}

fn pairs(m: &BTreeMap<String, String>) -> impl Iterator<Item = (&str, &str)> {
    m.iter().map(|(k, v)| (k.as_str(), v.as_str()))
}

/// A config that parses but refers to something that isn't there.
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid {
    /// Where the mistake is.
    pub part: Part,
    pub message: String,
}

/// A part of the config, for finding the file that set it.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// `modules.<id>`.
    Module(String),
    /// The screen with this name.
    Screen(String),
    /// `rotate:`.
    Rotate,
    /// Another top-level key, such as `color_theme`.
    Key(String),
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Invalid {}

/// What a write starts from when the config file doesn't exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// The default config: no config file was found at all, so the demo
    /// layout was on screen, and saving a setting keeps it.
    Demo,
    /// An empty file: other files (a project file) make up the config, so
    /// the demo was not on screen and must not be written in.
    Empty,
}

/// Set `theme:` in the config file, changing only that key's bytes so the
/// user's comments and layout stay. Creates the file if it doesn't exist.
/// `etag` is the version the change is based on (see [`set_top_level`]).
pub fn write_theme(path: &Path, missing: Missing, theme: Theme, etag: Option<&str>) -> Result<()> {
    let value = match theme {
        Theme::Auto => "auto",
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    set_top_level(
        path,
        missing,
        "theme",
        yaml_splice::Value::from(value),
        etag,
    )
}

/// Set `color_theme:` in the config file, like [`write_theme`].
pub fn write_color_theme(
    path: &Path,
    missing: Missing,
    name: &str,
    etag: Option<&str>,
) -> Result<()> {
    set_top_level(
        path,
        missing,
        "color_theme",
        yaml_splice::Value::from(name),
        etag,
    )
}

/// Replace `screens:` with these, keeping the rest of the file as written.
/// Comments inside the old `screens:` block are lost.
pub fn write_screens(
    path: &Path,
    missing: Missing,
    screens: &[ScreenLayout],
    etag: Option<&str>,
) -> Result<()> {
    write_screen_values(path, missing, screens_json(screens)?, etag)
}

/// Set `screens:` to these screens, as they are written in a config file.
/// `etag` as for [`set_top_level`].
pub fn write_screen_values(
    path: &Path,
    missing: Missing,
    screens: Vec<serde_json::Value>,
    etag: Option<&str>,
) -> Result<()> {
    set_top_level(
        path,
        missing,
        "screens",
        yaml_splice::Value::from(&serde_json::Value::Array(screens)),
        etag,
    )
}

/// Screens from the editor as config-file entries. Each needs a name, used once.
pub fn screens_json(screens: &[ScreenLayout]) -> Result<Vec<serde_json::Value>> {
    let mut names = std::collections::BTreeSet::new();
    for s in screens {
        if s.name.trim().is_empty() {
            bail!("a screen needs a name");
        }
        if !names.insert(s.name.as_str()) {
            bail!("screen name {:?} is used twice", s.name);
        }
    }
    Ok(screens
        .iter()
        .map(|s| {
            let tiles: Vec<serde_json::Value> = s
                .tiles
                .iter()
                .map(|t| serde_json::json!({ "module": t.tile_id, "size": t.size }))
                .collect();
            serde_json::json!({ "name": s.name, "tiles": tiles })
        })
        .collect())
}

/// Set one top-level key. A compare-and-swap write: `etag` is the version of
/// the file the change is based on (`Loaded::etag`), and the write is refused
/// when the file has changed since, or exists and no etag was sent, so a
/// stale view never overwrites newer edits. A missing file is created.
fn set_top_level(
    path: &Path,
    start: Missing,
    key: &str,
    value: yaml_splice::Value,
    etag: Option<&str>,
) -> Result<()> {
    let (source, missing) = read_source(path, start)?;
    let edit = yaml_splice::FieldEdit::Set {
        key: key.into(),
        value,
    };
    // Empty and comment-only files work: the key is appended.
    let out = yaml_splice::edit_yaml(&source, &[edit])
        .map_err(|e| anyhow::anyhow!("{e} in {}", path.display()))?;
    // Checked against the bytes the edit was made on, then again by
    // `write_cas` against the file as it is when written.
    let based_on = (!missing).then(|| etag_write::etag(&source));
    if let (Some(current), Some(sent)) = (&based_on, etag)
        && current != sent
    {
        return Err(stale(path));
    }
    // No etag means the view read no file; one exists now, so it is stale too.
    if based_on.is_some() && etag.is_none() {
        return Err(stale(path));
    }
    if !(out.changed || missing) {
        return Ok(());
    }
    write_over(path, &source, missing, &out.bytes)
}

/// The config file's bytes, or what `start` says when there is no file (true).
fn read_source(path: &Path, start: Missing) -> Result<(Vec<u8>, bool)> {
    match std::fs::read(path) {
        Ok(bytes) => Ok((bytes, false)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match start {
            Missing::Demo => Ok((DEFAULT_CONFIG.as_bytes().to_vec(), true)),
            Missing::Empty => Ok((vec![], true)),
        },
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Write `bytes` over the file read as `source` (`missing`: there was none),
/// refusing if it changed in between.
fn write_over(path: &Path, source: &[u8], missing: bool, bytes: &[u8]) -> Result<()> {
    let based_on = (!missing).then(|| etag_write::etag(source));
    match etag_write::write_cas(path, bytes, based_on.as_deref()) {
        Ok(_) => Ok(()),
        Err(etag_write::WriteError::Conflict { .. }) => Err(stale(path)),
        Err(etag_write::WriteError::ExistsWithoutEtag { .. }) => Err(stale(path)),
        Err(err) => Err(err.into()),
    }
}

fn stale(path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "{} changed since this view read it, so nothing was saved. \
         The view now shows the file as it is; make the change again.",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
# My dashboard
theme: dark
modules:
  cswap:
    plugin: [dashboard-cswap]
    refresh: 2m
  github-prs:
    size: wide
    demo: true
screens:
  - name: work
    tiles:
      - module: github-prs
  - name: system
    tiles:
      - module: sysperf
        size: large
      - module: cswap
rotate:
  interval: 30s
"#;

    fn available() -> Vec<String> {
        ["sysperf", "github-prs", "cswap"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn parses_example() {
        let c = parse(EXAMPLE).unwrap();
        assert_eq!(c.theme, Theme::Dark);
        assert_eq!(c.modules["cswap"].refresh, Some(Duration::from_secs(120)));
        assert_eq!(c.modules["github-prs"].size, Some(Size::Wide));
        assert!(c.modules["github-prs"].enabled);
        assert!(c.modules["github-prs"].demo);
        assert!(!c.modules["cswap"].demo);
        assert_eq!(c.screens[1].tiles[0].size, Some(Size::Large));
        let rotate = c.rotate.as_ref().unwrap();
        assert_eq!(rotate.interval, Duration::from_secs(30));
        assert_eq!(rotate.resume_after, Duration::from_secs(60));
        c.validate(&available()).unwrap();
    }

    #[test]
    fn screen_layouts_resolve_sizes() {
        let hints = |size, priority| LayoutHints {
            size,
            priority,
            min_size: None,
        };
        let running = vec![
            ("sysperf".to_string(), hints(Size::Medium, 100)),
            ("github-prs".to_string(), hints(Size::Medium, 10)),
            ("cswap".to_string(), hints(Size::Small, 50)),
        ];

        let c = parse(EXAMPLE).unwrap();
        let screens = c.screen_layouts(&running);
        assert_eq!(screens.len(), 2);
        // Module config size.
        assert_eq!(screens[0].tiles[0].size, Size::Wide);
        // Screen tile size beats the hint; otherwise the hint.
        assert_eq!(screens[1].tiles[0].size, Size::Large);
        assert_eq!(screens[1].tiles[1].size, Size::Small);

        // No screens: one, by priority.
        let screens = Config::default().screen_layouts(&running);
        let order: Vec<&str> = screens[0]
            .tiles
            .iter()
            .map(|t| t.tile_id.as_str())
            .collect();
        assert_eq!(order, ["sysperf", "cswap", "github-prs"]);

        // A screen tile for a module that isn't running is dropped.
        let screens = c.screen_layouts(&running[..1]);
        assert!(screens[0].tiles.is_empty());
    }

    #[test]
    fn rotate_settings_default_to_all_screens() {
        let c = parse(EXAMPLE).unwrap();
        let screens = c.screen_layouts(&[]);
        let r = c.rotate_settings(&screens).unwrap();
        assert_eq!(r.screens, ["work", "system"]);
        assert_eq!(r.interval_ms, 30_000);
        assert_eq!(r.resume_after_ms, 60_000);
        // One screen: nothing to rotate.
        assert!(c.rotate_settings(&screens[..1]).is_none());
    }

    #[test]
    fn write_screens_replaces_only_screens() {
        let dir =
            std::env::temp_dir().join(format!("dashboard-screens-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(
            &path,
            "# mine\ntheme: dark  # keep\nscreens:\n  - name: old\n    tiles: []\nmodules: {}\n",
        )
        .unwrap();
        let screens = vec![ScreenLayout {
            name: "new".into(),
            tiles: vec![TilePlacement {
                tile_id: "cpu".into(),
                size: Size::Wide,
            }],
        }];
        write_screens(&path, Missing::Demo, &screens, etag_of(&path).as_deref()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\ntheme: dark  # keep\n"), "{text}");
        let c = parse(&text).unwrap();
        assert_eq!(c.screens.len(), 1);
        assert_eq!(c.screens[0].tiles[0].module, "cpu");
        assert_eq!(c.screens[0].tiles[0].size, Some(Size::Wide));

        let dup = vec![screens[0].clone(), screens[0].clone()];
        assert!(write_screens(&path, Missing::Demo, &dup, etag_of(&path).as_deref()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_loads_the_demo() {
        let loaded = load(Some(Path::new("/nonexistent/dashboard/config.yaml"))).unwrap();
        assert!(!loaded.exists);
        assert_eq!(loaded.config.screens.len(), 8);
    }

    #[test]
    fn write_theme_keeps_comments() {
        let dir =
            std::env::temp_dir().join(format!("dashboard-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");

        std::fs::write(
            &path,
            "# My dashboard\ntheme: dark   # night mode\nscreens: []\n",
        )
        .unwrap();
        write_theme(
            &path,
            Missing::Demo,
            Theme::Light,
            etag_of(&path).as_deref(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# My dashboard\ntheme: light   # night mode\nscreens: []\n"
        );

        // Key missing: appended.
        std::fs::write(&path, "# only a comment\nscreens: []\n").unwrap();
        write_theme(&path, Missing::Demo, Theme::Dark, etag_of(&path).as_deref()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# only a comment\nscreens: []\ntheme: dark\n"
        );

        // No file yet: created from the default config, even if the value matches.
        std::fs::remove_file(&path).unwrap();
        write_theme(&path, Missing::Demo, Theme::Auto, None).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
        assert_eq!(
            parse(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .theme,
            Theme::Auto
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn etag_of(path: &Path) -> Option<String> {
        load(Some(path)).unwrap().etag
    }

    #[test]
    fn stale_writes_are_refused() {
        let dir = std::env::temp_dir().join(format!("dashboard-cas-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, "theme: dark\n").unwrap();
        let seen = etag_of(&path);

        // Someone edits the file after the view read it.
        std::fs::write(&path, "theme: dark  # edited\n").unwrap();
        let err = write_theme(&path, Missing::Demo, Theme::Light, seen.as_deref()).unwrap_err();
        assert!(
            err.to_string().contains("changed since this view read it"),
            "{err}"
        );
        // No etag (the view read no file) is refused too, once a file exists.
        let err = write_theme(&path, Missing::Demo, Theme::Light, None).unwrap_err();
        assert!(
            err.to_string().contains("changed since this view read it"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme: dark  # edited\n"
        );

        // With the current etag it goes through.
        write_theme(
            &path,
            Missing::Demo,
            Theme::Light,
            etag_of(&path).as_deref(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme: light  # edited\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_file_is_default() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert_eq!(parse("# nothing yet\n").unwrap(), Config::default());
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(parse("theme: dark\nthemes: light\n").is_err());
    }

    #[test]
    fn validate_catches_bad_references() {
        let c = parse("screens:\n  - name: a\n    tiles:\n      - module: nope\n").unwrap();
        assert_eq!(
            c.validate(&available()).unwrap_err().part,
            Part::Screen("a".into())
        );
        let c = parse("rotate:\n  interval: 10s\n  screens: [missing]\n").unwrap();
        assert_eq!(c.validate(&available()).unwrap_err().part, Part::Rotate);
        let c = parse("modules:\n  x:\n    builtin: cpu\n    package: hello\n").unwrap();
        assert_eq!(
            c.validate(&available()).unwrap_err().part,
            Part::Module("x".into())
        );
    }

    #[test]
    fn color_themes_resolve_and_validate() {
        let c = parse(
            "theme: dark
",
        )
        .unwrap();
        assert_eq!(c.color_theme().unwrap().name, "default");

        let c = parse(
            "color_theme: mine
color_themes:
  mine:
    base: paper
    \
             light:\n      accent: \"#C2410C\"\n    dark:\n      series-1: \"#abc\"\n",
        )
        .unwrap();
        c.validate(&available()).unwrap();
        let t = c.color_theme().unwrap();
        assert_eq!(t.name, "mine");
        assert_eq!(t.light.accent, "#c2410c");
        assert_eq!(t.dark.series_1, "#aabbcc");
        let paper = theme::builtin("paper").unwrap();
        assert_eq!(t.light.bg, paper.light.bg);
        assert_eq!(c.color_theme_names().last().unwrap(), "mine");

        let err = |yaml: &str| {
            let c = parse(yaml).unwrap();
            format!("{:#}", c.validate(&available()).unwrap_err())
        };
        assert!(err("color_theme: nope\n").contains("unknown theme \"nope\""));
        assert_eq!(
            err("color_themes:\n  x:\n    light:\n      ok: green\n"),
            "color_themes.x.light.ok: \"green\" is not a color; use #rrggbb or #rgb"
        );
        assert!(
            err("color_themes:\n  x:\n    dark:\n      okay: \"#fff\"\n")
                .starts_with("color_themes.x.dark.okay: unknown token")
        );
        assert!(err("color_themes:\n  x:\n    base: mine\n").contains("not a built-in theme"));
        assert!(err("color_themes:\n  paper: {}\n").contains("a built-in theme has this name"));
        assert!(parse("color_themes:\n  x:\n    lite: {}\n").is_err());
    }
}
