//! Runs modules from the config and keeps them in step with it: when the
//! config file changes, only modules whose settings changed are restarted, and
//! renderers get new `UiSettings`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use dashboard_config::packages::Packages;
use dashboard_config::{Config, ModuleConfig};
use dashboard_contract::{CatalogEntry, ClientMessage, ConfigInfo, ModuleSummary, UiSettings};
use dashboard_host::{Demo, Host, Module, Plugin, SpawnOptions};
use tokio::sync::{Mutex, mpsc, watch};

pub struct Runtime {
    pub host: Host,
    path: PathBuf,
    exists: bool,
    config: Config,
    error: Option<String>,
    settings: watch::Sender<UiSettings>,
}

impl Runtime {
    /// Load the config and start its modules. An invalid config at startup is an error.
    /// A module that fails to start is reported in the settings screen.
    pub async fn start(path: Option<&Path>) -> Result<(Self, watch::Receiver<UiSettings>)> {
        let loaded = dashboard_config::load(path)?;
        loaded.config.validate(&known_modules(&loaded.config))?;
        let (settings, rx) = watch::channel(placeholder_settings());
        let mut runtime = Self {
            host: Host::new(),
            path: loaded.path,
            exists: loaded.exists,
            config: loaded.config,
            error: None,
            settings,
        };
        runtime.error = runtime
            .sync_modules(None)
            .await
            .err()
            .map(|e| format!("{e:#}"));
        runtime.publish().await;
        Ok((runtime, rx))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Re-read the config file. On error the current config stays in effect
    /// and the error is shown in the settings.
    pub async fn reload(&mut self) {
        let result = dashboard_config::load(Some(&self.path)).and_then(|loaded| {
            loaded.config.validate(&known_modules(&loaded.config))?;
            Ok(loaded)
        });
        match result {
            Ok(loaded) => {
                let old = std::mem::replace(&mut self.config, loaded.config);
                self.exists = loaded.exists;
                self.error = self
                    .sync_modules(Some(&old))
                    .await
                    .err()
                    .map(|e| format!("{e:#}"));
            }
            Err(err) => self.error = Some(format!("{err:#}")),
        }
        self.publish().await;
    }

    /// Carry out an action from a renderer.
    pub async fn handle(&mut self, msg: ClientMessage) {
        match msg {
            ClientMessage::SetTheme { theme } => {
                let result = dashboard_config::write_theme(&self.path, theme);
                self.after_write("saving theme", result).await;
            }
            ClientMessage::SaveScreens { screens } => {
                let result = dashboard_config::write_screens(&self.path, &screens);
                self.after_write("saving screens", result).await;
            }
        }
    }

    /// Reload right after writing the config, rather than waiting for the file poll.
    async fn after_write(&mut self, what: &str, result: Result<()>) {
        match result {
            Ok(()) => self.reload().await,
            Err(err) => {
                self.error = Some(format!("{what}: {err:#}"));
                self.publish().await;
            }
        }
    }

    /// Start, stop or restart modules so the running set matches the config.
    /// With `old`, modules whose settings are unchanged keep running. A module
    /// that fails to start doesn't stop the others; the errors are returned together.
    async fn sync_modules(&self, old: Option<&Config>) -> Result<()> {
        let new = &self.config;
        let wanted = wanted_modules(new);
        let running: BTreeSet<String> = self
            .host
            .modules()
            .await
            .into_iter()
            .map(|m| m.tile_id)
            .collect();
        let changed = |id: &str| old.map(|o| o.module(id)) != Some(new.module(id));

        for id in &running {
            if !wanted.contains(id) || changed(id) {
                self.host.stop(id).await;
            }
        }
        let mut errors = vec![];
        let mut packages = None;
        for id in &wanted {
            let module_config = new.module(id);
            if !module_config.enabled || (running.contains(id) && !changed(id)) {
                continue;
            }
            let module: Box<dyn Module> = if module_config.demo {
                let packages = packages.get_or_insert_with(|| Packages::load(&self.path));
                match demo_module(id, &module_config, packages) {
                    Ok(m) => m,
                    Err(err) => {
                        errors.push(err);
                        continue;
                    }
                }
            } else if let Some(name) = &module_config.package {
                // Looked up when needed, so a newly installed package is found on reload.
                let packages = packages.get_or_insert_with(|| Packages::load(&self.path));
                match packages.get(name) {
                    Some(p) => Box::new(Plugin::new(id, p.argv()).in_dir(&p.dir)),
                    None => {
                        errors.push(format!(
                            "modules.{id}.package: no installed package {name:?} (see `dashboard widgets`)"
                        ));
                        continue;
                    }
                }
            } else {
                match plugin_argv(id, &module_config) {
                    // Relative paths in a plugin's argv are relative to the config file.
                    Some(argv) => match self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
                        Some(dir) => Box::new(Plugin::new(id, argv).in_dir(dir)),
                        None => Box::new(Plugin::new(id, argv)),
                    },
                    None => {
                        let name = module_config.builtin.as_deref().unwrap_or(id);
                        match dashboard_modules::builtin(name) {
                            Some(m) => m,
                            None => {
                                errors.push(format!(
                                    "modules.{id}.builtin: no built-in module {name:?}"
                                ));
                                continue;
                            }
                        }
                    }
                }
            };
            let options = SpawnOptions {
                refresh: module_config.refresh,
                demo: module_config.demo,
            };
            if let Err(err) = self
                .host
                .spawn(id, module, module_config.config, options)
                .await
            {
                errors.push(format!("starting module {id:?}: {err:#}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(errors.join("\n")))
        }
    }

    async fn publish(&self) {
        let modules = self.host.modules().await;
        let running: Vec<_> = modules
            .iter()
            .map(|m| (m.tile_id.clone(), m.manifest.layout.clone()))
            .collect();
        let screens = self.config.screen_layouts(&running);
        let settings = UiSettings {
            theme: self.config.theme,
            rotate: self.config.rotate_settings(&screens),
            screens,
            modules: modules
                .iter()
                .map(|m| ModuleSummary {
                    tile_id: m.tile_id.clone(),
                    name: m.manifest.name.clone(),
                    refresh_ms: m.interval.map(|d| d.as_millis() as u64),
                    refresh_overridden: self.config.module(&m.tile_id).refresh.is_some(),
                    commands: m
                        .manifest
                        .commands
                        .iter()
                        .map(|c| c.label.clone())
                        .collect(),
                    default_size: m.manifest.layout.size,
                })
                .collect(),
            config: ConfigInfo {
                path: self.path.display().to_string(),
                exists: self.exists,
                error: self.error.clone(),
            },
        };
        self.settings.send_replace(settings);
    }
}

/// Carry out renderer actions as they arrive. Returns the sender renderers use.
pub fn handle_actions(runtime: Arc<Mutex<Runtime>>) -> mpsc::UnboundedSender<ClientMessage> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            runtime.lock().await.handle(msg).await;
        }
    });
    tx
}

/// Reload when the config file changes. Polls its modification time and size
/// once a second: simple, and it works for files that editors replace, behind
/// symlinks, and on file systems where change events are unreliable.
pub fn watch_config(runtime: Arc<Mutex<Runtime>>, path: PathBuf) {
    tokio::spawn(async move {
        let mut last = fingerprint(&path);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let now = fingerprint(&path);
            if now != last {
                last = now;
                runtime.lock().await.reload().await;
            }
        }
    });
}

/// Modification time and length, or `None` when the file doesn't exist.
fn fingerprint(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The stand-in for a module with `demo: true`: the demo view from its catalog
/// entry. Nothing is started, so it needs no process, network or token.
fn demo_module(
    id: &str,
    config: &ModuleConfig,
    packages: &Packages,
) -> Result<Box<dyn Module>, String> {
    if let Some(name) = &config.package {
        return match packages.get(name) {
            Some(p) => Ok(Box::new(Demo::new(&p.entry()))),
            None => Err(format!(
                "modules.{id}.package: no installed package {name:?} (see `dashboard widgets`)"
            )),
        };
    }
    if config.plugin.is_some() {
        return Ok(Box::new(Demo::missing(id)));
    }
    let name = config.builtin.as_deref().unwrap_or(id);
    let entry = builtin_catalog()
        .into_iter()
        .find(|e| e.id == name)
        .ok_or_else(|| format!("modules.{id}.demo: no demo view for {name:?}"))?;
    Ok(Box::new(Demo::new(&entry)))
}

/// Plugins that ship inside the dashboard binary, run as `dashboard plugin <id>`.
const BUNDLED: &[&str] = &["cswap"];

/// How to run module `id` as a plugin: its configured command, or the bundled
/// plugin of that name. `None` means it's a built-in.
fn plugin_argv(id: &str, config: &ModuleConfig) -> Option<Vec<String>> {
    if let Some(argv) = &config.plugin {
        return Some(argv.clone());
    }
    BUNDLED.iter().find(|name| **name == id)?;
    let exe = std::env::current_exe().ok()?;
    Some(vec![exe.display().to_string(), "plugin".into(), id.into()])
}

/// Every module the config may name: built-ins, bundled plugins, and modules
/// configured as a plugin, a package, or a second copy of a built-in (`builtin:`).
pub fn known_modules(config: &Config) -> Vec<String> {
    let mut ids: Vec<String> = dashboard_modules::ids()
        .into_iter()
        .chain(BUNDLED.iter().copied())
        .map(|s| s.to_string())
        .collect();
    for (id, m) in &config.modules {
        if (m.plugin.is_some() || m.builtin.is_some() || m.package.is_some()) && !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// A problem for each `modules.<id>.package` that no installed package matches.
pub fn missing_packages(config: &Config, packages: &Packages) -> Vec<String> {
    config
        .modules
        .iter()
        .filter_map(|(id, m)| {
            let name = m.package.as_ref()?;
            packages.get(name).is_none().then(|| {
                format!(
                    "modules.{id}.package: no installed package {name:?} (see `dashboard widgets`)"
                )
            })
        })
        .collect()
}

/// Everything that can be added: built-ins, bundled plugins, and the plugin
/// packages installed for the config at `config_path`. Also returns problems
/// with package folders that couldn't be read.
pub fn catalog(config_path: &Path) -> (Vec<CatalogEntry>, Vec<String>) {
    let mut entries = builtin_catalog();
    let packages = Packages::load(config_path);
    entries.extend(packages.installed.iter().map(|p| p.entry()));
    (entries, packages.errors)
}

/// Built-ins and bundled plugins: what ships inside the binary.
fn builtin_catalog() -> Vec<CatalogEntry> {
    let mut entries = dashboard_modules::catalog();
    entries.push(dashboard_cswap::catalog());
    entries
}

/// The modules to run: only those the config names, under `modules:` or in a
/// screen's tiles.
fn wanted_modules(config: &Config) -> Vec<String> {
    let named = |id: &String| {
        config.modules.contains_key(id)
            || config
                .screens
                .iter()
                .any(|s| s.tiles.iter().any(|t| &t.module == id))
    };
    known_modules(config).into_iter().filter(named).collect()
}

fn placeholder_settings() -> UiSettings {
    UiSettings {
        theme: Default::default(),
        screens: vec![],
        modules: vec![],
        rotate: None,
        config: ConfigInfo::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_only_modules_the_config_names() {
        // Works in any build: names the first built-in this build has, if any.
        let builtins = dashboard_modules::ids();
        let first = builtins.first().map(|id| format!("  {id}: {{}}\n"));
        let config = dashboard_config::parse(&format!(
            "modules:\n{}  not-in-build: {{}}\n  ci:\n    plugin: [python3, ci.py]\n\
             screens:\n  - name: a\n    tiles:\n      - module: cswap\n",
            first.unwrap_or_default()
        ))
        .unwrap();
        let mut expected: Vec<&str> = builtins.first().copied().into_iter().collect();
        expected.extend(["cswap", "ci"]);
        // A `modules:` entry the build doesn't have is left out, not an error.
        assert_eq!(wanted_modules(&config), expected);
        // Known but not named: still valid to add later, not started.
        let known = known_modules(&config);
        for id in &builtins {
            assert!(known.contains(&id.to_string()), "{id}");
        }
        assert!(!known.contains(&"not-in-build".to_string()));
    }

    #[test]
    fn reports_packages_that_are_not_installed() {
        let config = dashboard_config::parse(
            "modules:\n  a:\n    package: hello\n  b:\n    package: nope\n  c: {}\n",
        )
        .unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
        let packages = Packages::load_from(&[dir]);
        assert_eq!(
            missing_packages(&config, &packages),
            ["modules.b.package: no installed package \"nope\" (see `dashboard widgets`)"]
        );
    }

    #[tokio::test]
    async fn demo_uses_the_catalog_view() {
        let config = dashboard_config::parse(
            r"
modules:
  prs: { builtin: github-prs, demo: true }
  cswap: { demo: true }
  cmd: { plugin: [nope], demo: true }
  pkg: { package: nope, demo: true }
",
        )
        .unwrap();
        let packages = Packages::load_from(&[]);
        let catalog = builtin_catalog();
        // github-prs is only in builds with its feature; cswap is always bundled.
        let mut cases = vec![("cswap", "cswap")];
        if cfg!(feature = "module-github-prs") {
            cases.push(("prs", "github-prs"));
        }
        for (id, name) in cases {
            let mut m = demo_module(id, &config.module(id), &packages).unwrap();
            let manifest = m.init(serde_json::Value::Null).await.unwrap();
            let entry = catalog.iter().find(|e| e.id == name).unwrap();
            assert_eq!(manifest.name, entry.name);
            assert_eq!(m.render().await.unwrap(), entry.demo);
        }
        // A plugin given by command has no catalog entry: a placeholder.
        let mut m = demo_module("cmd", &config.module("cmd"), &packages).unwrap();
        assert_eq!(m.render().await.unwrap().summary, "no demo view");
        assert!(demo_module("pkg", &config.module("pkg"), &packages).is_err());
    }
}
