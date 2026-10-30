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
use dashboard_contract::{LayoutHints, RotateSettings, ScreenLayout, Size, Theme, TilePlacement};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod packages;

pub const TOOL: &str = "dashboard";
const VENDOR: &str = "dev.sksizer";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub theme: Theme,
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
}

/// Used when there is no config file: the demo, so a first run shows what
/// the dashboard can do. Saving a setting from the UI writes this out first.
pub const DEFAULT_CONFIG: &str = include_str!("../../../examples/demo.yaml");

/// Default config path: `<config home>/dashboard/config.yaml`, XDG layout on
/// every OS (`~/.config/dashboard/config.yaml` on macOS too). Moved by
/// `DASHBOARD_MACHINE_CONFIG`.
pub fn default_path() -> Result<PathBuf> {
    let dirs = TieredDirs::from_env(
        VENDOR,
        &env_vars!("DASHBOARD"),
        ResolveOptions::new().with_convention(Convention::Xdg),
    )?;
    Ok(MachineConfigFile::<Config>::new(TOOL, &dirs)?.path())
}

/// Where a background `serve` keeps its record (the runtime dir: pidfiles,
/// locks) and its log (the logs dir), e.g. `~/.local/state/dashboard/...`.
pub fn run_dirs() -> Result<(PathBuf, PathBuf)> {
    let dirs = TieredDirs::from_env(
        VENDOR,
        &env_vars!("DASHBOARD"),
        ResolveOptions::new().with_convention(Convention::Xdg),
    )?;
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
        });
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let config = parse(&text).with_context(|| format!("in {}", path.display()))?;
    Ok(Loaded {
        config,
        path,
        exists: true,
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

    /// Check references between sections. `available` is every module ID the
    /// host can run (built-ins plus configured plugins).
    pub fn validate(&self, available: &[String]) -> Result<()> {
        for (id, m) in &self.modules {
            if m.plugin.as_ref().is_some_and(|argv| argv.is_empty()) {
                bail!("modules.{id}.plugin: needs at least a program name");
            }
            let ways = [m.plugin.is_some(), m.builtin.is_some(), m.package.is_some()];
            if ways.iter().filter(|w| **w).count() > 1 {
                bail!("modules.{id}: set one of `plugin`, `builtin` or `package`");
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for screen in &self.screens {
            if !names.insert(screen.name.as_str()) {
                bail!("screens: name {:?} is used twice", screen.name);
            }
            for tile in &screen.tiles {
                if !available.contains(&tile.module) {
                    bail!(
                        "screen {:?}: unknown module {:?} (available: {}; it may be left out of this build, see docs/modules.md)",
                        screen.name,
                        tile.module,
                        available.join(", ")
                    );
                }
            }
        }
        if let Some(rotate) = &self.rotate {
            if rotate.interval.is_zero() {
                bail!("rotate.interval must be more than zero");
            }
            for name in rotate.screens.iter().flatten() {
                if !names.contains(name.as_str()) {
                    bail!("rotate.screens: unknown screen {name:?}");
                }
            }
        }
        Ok(())
    }
}

/// Set `theme:` in the config file, changing only that key's bytes so the
/// user's comments and layout stay. Creates the file if it doesn't exist.
pub fn write_theme(path: &Path, theme: Theme) -> Result<()> {
    let value = match theme {
        Theme::Auto => "auto",
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    set_top_level(path, "theme", yaml_splice::Value::from(value))
}

/// Replace `screens:` with these, keeping the rest of the file as written.
/// Comments inside the old `screens:` block are lost.
pub fn write_screens(path: &Path, screens: &[ScreenLayout]) -> Result<()> {
    let mut names = std::collections::BTreeSet::new();
    for s in screens {
        if s.name.trim().is_empty() {
            bail!("a screen needs a name");
        }
        if !names.insert(s.name.as_str()) {
            bail!("screen name {:?} is used twice", s.name);
        }
    }
    let json: serde_json::Value = screens
        .iter()
        .map(|s| {
            let tiles: Vec<serde_json::Value> = s
                .tiles
                .iter()
                .map(|t| serde_json::json!({ "module": t.tile_id, "size": t.size }))
                .collect();
            serde_json::json!({ "name": s.name, "tiles": tiles })
        })
        .collect();
    set_top_level(path, "screens", yaml_splice::Value::from(&json))
}

fn set_top_level(path: &Path, key: &str, value: yaml_splice::Value) -> Result<()> {
    let (source, missing) = match std::fs::read(path) {
        Ok(bytes) => (bytes, false),
        // Start from what's on screen, so saving a setting keeps the demo layout.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (DEFAULT_CONFIG.as_bytes().to_vec(), true)
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let edit = yaml_splice::FieldEdit::Set {
        key: key.into(),
        value,
    };
    // Empty and comment-only files work: the key is appended.
    let out = yaml_splice::edit_yaml(&source, &[edit])
        .map_err(|e| anyhow::anyhow!("{e} in {}", path.display()))?;
    if out.changed || missing {
        write_atomic(path, &out.bytes)?;
    }
    Ok(())
}

/// Write via a temporary file and rename, so readers never see a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
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
        write_screens(&path, &screens).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\ntheme: dark  # keep\n"), "{text}");
        let c = parse(&text).unwrap();
        assert_eq!(c.screens.len(), 1);
        assert_eq!(c.screens[0].tiles[0].module, "cpu");
        assert_eq!(c.screens[0].tiles[0].size, Some(Size::Wide));

        let dup = vec![screens[0].clone(), screens[0].clone()];
        assert!(write_screens(&path, &dup).is_err());
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
        write_theme(&path, Theme::Light).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# My dashboard\ntheme: light   # night mode\nscreens: []\n"
        );

        // Key missing: appended.
        std::fs::write(&path, "# only a comment\nscreens: []\n").unwrap();
        write_theme(&path, Theme::Dark).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# only a comment\nscreens: []\ntheme: dark\n"
        );

        // No file yet: created from the default config, even if the value matches.
        std::fs::remove_file(&path).unwrap();
        write_theme(&path, Theme::Auto).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
        assert_eq!(
            parse(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .theme,
            Theme::Auto
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
        assert!(c.validate(&available()).is_err());
        let c = parse("rotate:\n  interval: 10s\n  screens: [missing]\n").unwrap();
        assert!(c.validate(&available()).is_err());
        let c = parse("modules:\n  x:\n    builtin: cpu\n    package: hello\n").unwrap();
        assert!(c.validate(&available()).is_err());
    }
}
