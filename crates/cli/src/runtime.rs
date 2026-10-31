//! Runs modules from the config and keeps them in step with it: when the
//! config file changes, only modules whose settings changed are restarted, and
//! renderers get new `UiSettings`. The config is the merge of its layers
//! (see `dashboard_config::layers`); any of their files changing reloads it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use dashboard_config::dashboards::Registry;
use dashboard_config::layers::{self, LayerKind, Layered, Sources};
use dashboard_config::packages::{Packages, market_entries, market_index_path};
use dashboard_config::{Config, ModuleConfig, NewModule};
use dashboard_contract::{
    AddOutcome, CatalogEntry, ClientMessage, ConfigInfo, DashboardSummary, EntryKind,
    ModuleSummary, Size, UiSettings,
};
use dashboard_host::{Demo, Host, Module, Plugin, SpawnOptions};
use tokio::sync::{Mutex, mpsc, watch};

pub struct Runtime {
    pub host: Host,
    sources: Sources,
    loaded: Layered,
    config: Config,
    error: Option<String>,
    added: Option<AddOutcome>,
    /// A note from the last save, kept across reloads (the save itself
    /// triggers one) until the next save or switch.
    notice: Option<String>,
    /// The files in use, shared with the web server's catalog, which lists
    /// the packages installed next to each of them.
    layer_paths: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    settings: watch::Sender<UiSettings>,
}

impl Runtime {
    /// Load the config and start its modules. An invalid config at startup is an error.
    /// A module that fails to start is reported in the settings screen.
    pub async fn start(sources: Sources) -> Result<(Self, watch::Receiver<UiSettings>)> {
        let loaded = load_valid(&sources)?;
        let (settings, rx) = watch::channel(placeholder_settings());
        let mut runtime = Self {
            host: Host::new(),
            sources,
            config: loaded.config.clone(),
            loaded,
            error: None,
            added: None,
            notice: None,
            layer_paths: Arc::default(),
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

    /// The config files in use, kept current across reloads and switches.
    pub fn layer_paths(&self) -> Arc<std::sync::Mutex<Vec<PathBuf>>> {
        self.layer_paths.clone()
    }

    /// The file settings from a renderer are written to.
    fn path(&self) -> &Path {
        self.loaded.write_path()
    }

    /// The files to watch: every layer, the saved-dashboards folder so the
    /// switcher sees new ones, a project file that may appear in the working
    /// directory or be trusted, and the trust list.
    pub fn watched(&self) -> Vec<PathBuf> {
        let mut paths = self.loaded.paths();
        if let Some(dir) = registry(&self.sources).ok().map(|r| r.dir) {
            paths.push(dir);
        }
        let candidate = self
            .sources
            .project_from
            .as_ref()
            .map(|d| d.join(layers::PROJECT_FILE));
        let trust = self.sources.trust().ok().map(|t| t.path);
        for path in [candidate, self.loaded.untrusted.clone(), trust]
            .into_iter()
            .flatten()
        {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
    }

    /// The files in use, and a skipped project file, for the startup message.
    pub fn describe(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .loaded
            .layers
            .iter()
            .map(|l| format!("{}: {}", l.kind.label(), l.path.display()))
            .collect();
        lines.extend(self.loaded.untrusted_notice());
        lines
    }

    /// Re-read the config files. On error the current config stays in effect
    /// and the error is shown in the settings.
    pub async fn reload(&mut self) {
        match load_valid(&self.sources) {
            Ok(loaded) => self.apply(loaded).await,
            Err(err) => self.error = Some(format!("{err:#}")),
        }
        self.publish().await;
    }

    /// Run what `loaded` asks for, restarting only the modules that changed.
    async fn apply(&mut self, loaded: Layered) {
        let old = std::mem::replace(&mut self.config, loaded.config.clone());
        self.loaded = loaded;
        self.error = self
            .sync_modules(Some(&old))
            .await
            .err()
            .map(|e| format!("{e:#}"));
    }

    /// Show another saved dashboard (`None`: none). If it can't be loaded,
    /// the current one stays and the error is shown.
    async fn switch(&mut self, name: Option<String>) {
        let sources = Sources {
            dashboard: name,
            ..self.sources.clone()
        };
        match load_valid(&sources) {
            Ok(loaded) => {
                self.notice = None;
                self.sources = sources;
                self.apply(loaded).await;
            }
            Err(err) => self.error = Some(format!("switching dashboard: {err:#}")),
        }
        self.publish().await;
    }

    /// Carry out an action from a renderer.
    pub async fn handle(&mut self, msg: ClientMessage) {
        match msg {
            ClientMessage::SetTheme { theme, etag } => {
                let result = dashboard_config::write_theme(self.path(), theme, etag.as_deref());
                self.after_write("saving theme", result).await;
            }
            ClientMessage::SaveScreens { screens, etag } => {
                self.notice = None;
                // The project file's screens are edited there, not copied here.
                // Only content is compared: moving a project screen among the
                // user's own changes nothing that can be saved, so it is not noted.
                let (project, own): (Vec<_>, Vec<_>) = screens
                    .into_iter()
                    .partition(|s| self.loaded.project_screens.contains(&s.name));
                let shown = self.settings.borrow().screens.clone();
                let project_changed = project.iter().any(|s| !shown.iter().any(|old| old == s));
                let result = dashboard_config::screens_json(&own).and_then(|mut json| {
                    // Screens of this file that a project screen hides stay in it.
                    self.loaded.restore_shadowed(&mut json);
                    dashboard_config::write_screen_values(self.path(), json, etag.as_deref())
                });
                let project_file = self.loaded.layer(LayerKind::Project).map(Path::to_path_buf);
                let saved = result.is_ok();
                self.after_write("saving screens", result).await;
                if saved
                    && project_changed
                    && let Some(file) = project_file
                {
                    self.notice = Some(format!(
                        "Changes to the project's screens were not saved: edit them in {}.",
                        file.display()
                    ));
                    self.publish().await;
                }
            }
            ClientMessage::AddModule {
                module_id,
                entry,
                kind,
                config,
                demo,
                screen,
                size,
            } => {
                let result = self.add_module(&AddRequest {
                    module_id: module_id.clone(),
                    entry,
                    kind,
                    config,
                    demo,
                    screen,
                    size,
                });
                // Reported on its own, not as the config error: a refused add
                // leaves the config as it was.
                self.added = Some(AddOutcome {
                    module_id,
                    error: result.as_ref().err().map(|e| format!("{e:#}")),
                });
                match result {
                    Ok(()) => self.reload().await,
                    Err(_) => self.publish().await,
                }
            }
            ClientMessage::SwitchDashboard { name } => self.switch(name).await,
        }
    }

    /// Write a catalog entry's `modules:` entry, and its tile when the config
    /// has screens. The reload that follows starts it.
    fn add_module(&self, req: &AddRequest) -> Result<()> {
        let entry = find_entry(&self.loaded.paths(), &req.entry, req.kind)?;
        let module = new_module(&self.config, &entry, req)?;
        // Without `screens:`, every running module shows on one screen already.
        let tile = match &req.screen {
            Some(name) if !self.config.screens.is_empty() => Some((name.as_str(), req.size)),
            _ => None,
        };
        dashboard_config::add_module(self.path(), &module, tile)
    }

    /// Reload right after writing the config, rather than waiting for the
    /// file poll. After a refused write too: it may have been refused because
    /// the file changed, and the renderer should show the file as it is now.
    async fn after_write(&mut self, what: &str, result: Result<()>) {
        self.reload().await;
        if let Err(err) = result {
            self.error = Some(format!("{what}: {err:#}"));
            self.publish().await;
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
                let packages =
                    packages.get_or_insert_with(|| Packages::load_for(&self.loaded.paths()));
                match demo_module(id, &module_config, packages) {
                    Ok(m) => m,
                    Err(err) => {
                        errors.push(err);
                        continue;
                    }
                }
            } else if let Some(name) = &module_config.package {
                // Looked up when needed, so a newly installed package is found on reload.
                let packages =
                    packages.get_or_insert_with(|| Packages::load_for(&self.loaded.paths()));
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
                    // Relative paths in a plugin's argv are relative to the
                    // config file that set it.
                    Some(argv) => match self
                        .loaded
                        .module_dirs
                        .get(id)
                        .filter(|d| !d.as_os_str().is_empty())
                    {
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
        *self.layer_paths.lock().expect("layer paths lock") = self.loaded.paths();
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
                path: self.path().display().to_string(),
                exists: self.loaded.exists,
                error: {
                    let lines: Vec<String> = [
                        self.loaded.untrusted_notice(),
                        self.notice.clone(),
                        self.error.clone(),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    (!lines.is_empty()).then(|| lines.join("\n"))
                },
                added: self.added.clone(),
                dashboard: self.loaded.dashboard.clone(),
                dashboards: registry(&self.sources)
                    .map(|r| r.list().0)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| DashboardSummary {
                        name: s.name,
                        description: s.description,
                    })
                    .collect(),
                layers: self
                    .loaded
                    .layers
                    .iter()
                    .map(|l| format!("{}: {}", l.kind.label(), l.path.display()))
                    .collect(),
                etag: self.loaded.etag.clone(),
            },
        };
        self.settings.send_replace(settings);
    }
}

/// The fields of `ClientMessage::AddModule`.
struct AddRequest {
    module_id: String,
    entry: String,
    kind: EntryKind,
    config: serde_json::Value,
    demo: bool,
    screen: Option<String>,
    size: Size,
}

/// The catalog entry `id` of `kind`, looked up without reading the whole
/// catalog. A package only in the marketplace index is an error.
fn find_entry(config_paths: &[PathBuf], id: &str, kind: EntryKind) -> Result<CatalogEntry> {
    // Bundled plugins are in the built-in catalog, with kind `Plugin`.
    let builtin = builtin_catalog()
        .into_iter()
        .find(|e| e.id == id && e.kind == kind);
    let found = match kind {
        EntryKind::Builtin => builtin,
        EntryKind::Plugin if builtin.is_some() => builtin,
        EntryKind::Plugin => {
            let packages = Packages::load_for(config_paths);
            match packages.get(id) {
                Some(p) => Some(p.entry()),
                None => {
                    let in_market = market_index_path()
                        .and_then(|path| market_entries(&path, &packages))
                        .is_ok_and(|(market, _)| market.iter().any(|e| e.id == id));
                    if in_market {
                        anyhow::bail!(
                            "{id:?} is in the marketplace but not installed; install it first"
                        );
                    }
                    None
                }
            }
        }
    };
    found.ok_or_else(|| anyhow::anyhow!("{id:?} is not in the catalog"))
}

/// The `modules:` entry for adding catalog `entry` as `req.module_id`.
fn new_module(config: &Config, entry: &CatalogEntry, req: &AddRequest) -> Result<NewModule> {
    let id = req.module_id.trim();
    let used = config.modules.contains_key(id)
        || config
            .screens
            .iter()
            .any(|s| s.tiles.iter().any(|t| t.module == id));
    if used {
        anyhow::bail!("{id:?} is already in the config; pick another ID");
    }
    // Another built-in's ID would hide that built-in.
    let builtin_ids: Vec<&str> = dashboard_modules::ids()
        .into_iter()
        .chain(BUNDLED.iter().copied())
        .collect();
    if id != entry.id && builtin_ids.contains(&id) {
        anyhow::bail!("{id:?} is a built-in module's ID; pick another");
    }
    let mut module = NewModule {
        id: id.to_string(),
        builtin: None,
        package: None,
        demo: req.demo,
        size: None,
        config: req.config.clone(),
    };
    let bundled = entry.source == "bundled" && BUNDLED.contains(&entry.id.as_str());
    match entry.kind {
        _ if bundled => {
            if id != entry.id {
                anyhow::bail!(
                    "{:?} is a bundled plugin; it runs only as {:?}",
                    entry.id,
                    entry.id
                );
            }
        }
        EntryKind::Plugin => module.package = Some(entry.id.clone()),
        EntryKind::Builtin if id != entry.id => module.builtin = Some(entry.id.clone()),
        EntryKind::Builtin => {}
    }
    // With no screens the size can only go on the module.
    if config.screens.is_empty() && req.size != entry.default_size {
        module.size = Some(req.size);
    }
    Ok(module)
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

/// Reload when a config file changes. Polls their modification times and
/// sizes once a second: simple, and it works for files that editors replace,
/// behind symlinks, and on file systems where change events are unreliable.
pub fn watch_config(runtime: Arc<Mutex<Runtime>>) {
    tokio::spawn(async move {
        let mut last = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let paths = runtime.lock().await.watched();
            let now: Vec<_> = paths.iter().map(|p| (p.clone(), fingerprint(p))).collect();
            // The first tick, and a switch to another dashboard, only take note.
            let same_files = last
                .as_ref()
                .is_some_and(|l: &Vec<(PathBuf, _)>| l.iter().map(|(p, _)| p).eq(paths.iter()));
            if same_files && last.as_ref() != Some(&now) {
                runtime.lock().await.reload().await;
            }
            last = Some(now);
        }
    });
}

/// Load the layers and check the merged config.
pub fn load_valid(sources: &Sources) -> Result<Layered> {
    let loaded = layers::load(sources)?;
    loaded.config.validate(&known_modules(&loaded.config))?;
    Ok(loaded)
}

/// The saved-dashboards folder for these sources.
pub fn registry(sources: &Sources) -> Result<Registry> {
    match &sources.registry {
        Some(dir) => Ok(Registry::new(dir)),
        None => Registry::open_default(),
    }
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

/// Everything known: built-ins, bundled plugins, the plugin packages
/// installed for these config files (the layers), and marketplace packages
/// from the cached index that aren't installed. Also returns problems with
/// package folders or the index that couldn't be read.
pub fn catalog(config_paths: &[PathBuf]) -> (Vec<CatalogEntry>, Vec<String>) {
    let mut entries = builtin_catalog();
    let packages = Packages::load_for(config_paths);
    entries.extend(packages.installed.iter().map(|p| p.entry()));
    let mut errors = packages.errors.clone();
    match market_index_path().and_then(|path| market_entries(&path, &packages)) {
        Ok((market, problems)) => {
            entries.extend(market);
            errors.extend(problems);
        }
        Err(err) => errors.push(format!("marketplace index: {err:#}")),
    }
    (entries, errors)
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

    /// Machine file, a saved dashboard and a project file, in a temp folder.
    #[tokio::test]
    async fn switches_dashboards_and_writes_to_the_right_file() {
        let dir = std::env::temp_dir().join(format!("dashboard-runtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dashboards")).unwrap();
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let machine = dir.join("config.yaml");
        // Demo views only: nothing is started, no network.
        std::fs::write(&machine, "modules:\n  cswap: { demo: true }\n").unwrap();
        let work = dir.join("dashboards/work.yaml");
        std::fs::write(
            &work,
            "description: Day job\nmodules:\n  ci: { plugin: [nope], demo: true }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("repo/.dashboard.yaml"),
            "screens:\n  - name: repo\n    tiles: [{ module: cswap }]\n",
        )
        .unwrap();
        let trust = dir.join("trusted-projects");
        dashboard_config::trust::Trust::new(&trust)
            .allow(&dir.join("repo/.dashboard.yaml"))
            .unwrap();
        let sources = Sources {
            machine: Some(machine.clone()),
            dashboard: None,
            project_from: Some(dir.join("repo")),
            registry: Some(dir.join("dashboards")),
            trust: Some(trust),
        };
        let (mut runtime, rx) = Runtime::start(sources).await.unwrap();
        let ids = |r: &Runtime| {
            let host = r.host.clone();
            async move {
                let mut ids: Vec<String> = host
                    .modules()
                    .await
                    .into_iter()
                    .map(|m| m.tile_id)
                    .collect();
                ids.sort();
                ids
            }
        };
        assert_eq!(ids(&runtime).await, ["cswap"]);
        let config = rx.borrow().config.clone();
        assert_eq!(config.dashboard, None);
        assert_eq!(
            config.dashboards,
            [DashboardSummary {
                name: "work".into(),
                description: Some("Day job".into())
            }]
        );
        assert_eq!(config.layers.len(), 2);
        assert_eq!(config.path, machine.display().to_string());

        runtime
            .handle(ClientMessage::SwitchDashboard {
                name: Some("work".into()),
            })
            .await;
        assert_eq!(ids(&runtime).await, ["ci", "cswap"]);
        let config = rx.borrow().config.clone();
        assert_eq!(config.dashboard.as_deref(), Some("work"));
        assert_eq!(config.path, work.display().to_string());

        // Saving screens writes to the dashboard, leaving out the project's.
        let screens = rx.borrow().screens.clone();
        assert_eq!(screens[0].name, "repo");
        let mut edited = screens.clone();
        edited.push(dashboard_contract::ScreenLayout {
            name: "mine".into(),
            tiles: vec![],
        });
        let etag = rx.borrow().config.etag.clone();
        runtime
            .handle(ClientMessage::SaveScreens {
                screens: edited,
                etag,
            })
            .await;
        let saved = dashboard_config::parse(&std::fs::read_to_string(&work).unwrap()).unwrap();
        let names: Vec<&str> = saved.screens.iter().map(|s| s.name.as_str()).collect();
        assert!(!names.contains(&"repo"), "{names:?}");
        assert!(names.contains(&"mine"), "{names:?}");
        assert!(!std::fs::read_to_string(&machine).unwrap().contains("mine"));

        // An unknown dashboard: an error, and the current one stays.
        runtime
            .handle(ClientMessage::SwitchDashboard {
                name: Some("nope".into()),
            })
            .await;
        let config = rx.borrow().config.clone();
        assert_eq!(config.dashboard.as_deref(), Some("work"));
        assert!(config.error.unwrap().contains("nope"));

        runtime
            .handle(ClientMessage::SwitchDashboard { name: None })
            .await;
        assert_eq!(ids(&runtime).await, ["cswap"]);
        assert_eq!(rx.borrow().config.error, None);
        for id in ids(&runtime).await {
            runtime.host.stop(&id).await;
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A project screen with the name of one of the user's: saving keeps the
    /// user's, and says that edits to the project's are not saved.
    #[tokio::test]
    async fn saving_keeps_screens_a_project_screen_hides() {
        let dir = std::env::temp_dir().join(format!("dashboard-shadow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo")).unwrap();
        let machine = dir.join("config.yaml");
        std::fs::write(
            &machine,
            "modules:\n  cswap: { demo: true }\nscreens:\n  - name: main\n    tiles: []\n  \
             - name: system\n    tiles: [{ module: cswap }]\n",
        )
        .unwrap();
        let project = dir.join("repo/.dashboard.yaml");
        std::fs::write(
            &project,
            "screens:\n  - name: repo\n    tiles: []\n  - name: system\n    tiles: []\n",
        )
        .unwrap();
        let trust = dir.join("trusted-projects");
        dashboard_config::trust::Trust::new(&trust)
            .allow(&project)
            .unwrap();
        let (mut runtime, rx) = Runtime::start(Sources {
            machine: Some(machine.clone()),
            project_from: Some(dir.join("repo")),
            registry: Some(dir.join("dashboards")),
            trust: Some(trust),
            ..Default::default()
        })
        .await
        .unwrap();
        let screens = rx.borrow().screens.clone();
        let names: Vec<&str> = screens.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["repo", "system", "main"]);

        // Unchanged: the file keeps both of its screens, and there is no notice.
        let etag = rx.borrow().config.etag.clone();
        runtime
            .handle(ClientMessage::SaveScreens {
                screens: screens.clone(),
                etag,
            })
            .await;
        let saved = dashboard_config::parse(&std::fs::read_to_string(&machine).unwrap()).unwrap();
        let names: Vec<&str> = saved.screens.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["main", "system"]);
        assert_eq!(saved.screens[1].tiles[0].module, "cswap");
        assert_eq!(rx.borrow().config.error, None);

        // A tile added to a project screen: not saved, and the user is told.
        let mut edited = screens;
        edited[0].tiles.push(dashboard_contract::TilePlacement {
            tile_id: "cswap".into(),
            size: dashboard_contract::Size::Wide,
        });
        let etag = rx.borrow().config.etag.clone();
        runtime
            .handle(ClientMessage::SaveScreens {
                screens: edited,
                etag,
            })
            .await;
        let error = rx.borrow().config.error.clone().unwrap();
        assert!(error.contains("not saved"), "{error}");
        assert!(
            !std::fs::read_to_string(&machine)
                .unwrap()
                .contains("name: repo")
        );
        // The file watcher reloads after the write; the note stays.
        runtime.reload().await;
        let error = rx.borrow().config.error.clone().unwrap();
        assert!(error.contains("not saved"), "{error}");
        for m in runtime.host.modules().await {
            runtime.host.stop(&m.tile_id).await;
        }
        std::fs::remove_dir_all(&dir).unwrap();
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

    #[test]
    fn new_module_picks_builtin_package_or_bundled() {
        let config = dashboard_config::parse(
            "modules:\n  taken: {}\nscreens:\n  - name: a\n    tiles:\n      - module: cswap\n",
        )
        .unwrap();
        let mut entries = builtin_catalog();
        let packages = Packages::load_from(&[
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins")
        ]);
        entries.extend(packages.installed.iter().map(|p| p.entry()));
        let add = |module_id: &str, entry: &str, kind| {
            let req = AddRequest {
                module_id: module_id.into(),
                entry: entry.into(),
                kind,
                config: serde_json::json!({"name": "x"}),
                demo: true,
                screen: Some("a".into()),
                size: Size::Small,
            };
            let entry = entries
                .iter()
                .find(|e| e.id == entry && e.kind == kind)
                .unwrap();
            new_module(&config, entry, &req)
        };
        let m = add("hi", "hello", EntryKind::Plugin).unwrap();
        assert_eq!(m.package.as_deref(), Some("hello"));
        assert!(m.demo);
        // Screens exist, so the size goes on the tile, not the module.
        assert_eq!(m.size, None);
        assert!(add("taken", "hello", EntryKind::Plugin).is_err());
        // cswap is on a screen already, and can't run under another ID.
        assert!(add("cswap", "cswap", EntryKind::Plugin).is_err());
        assert!(add("c2", "cswap", EntryKind::Plugin).is_err());
        assert!(find_entry(&[PathBuf::from("x.yaml")], "nope", EntryKind::Builtin).is_err());
        if let Some(first) = dashboard_modules::ids().first() {
            let m = add("copy", first, EntryKind::Builtin).unwrap();
            assert_eq!(m.builtin.as_deref(), Some(*first));
            let m = add(first, first, EntryKind::Builtin).unwrap();
            assert_eq!(m.builtin, None);
        }
    }

    #[tokio::test]
    async fn add_module_writes_the_config_and_starts_it() {
        let dir =
            std::env::temp_dir().join(format!("dashboard-runtime-add-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(
            &path,
            "# mine\nmodules:\n  other: { builtin: cswap, demo: true }  # keep\n\
             screens:\n  - name: main\n    tiles:\n      - module: other\n",
        )
        .unwrap();
        let (mut runtime, settings) = Runtime::start(Sources {
            machine: Some(path.clone()),
            registry: Some(dir.join("dashboards")),
            trust: Some(dir.join("trust.json")),
            ..Default::default()
        })
        .await
        .unwrap();
        let add = |module_id: &str| ClientMessage::AddModule {
            module_id: module_id.into(),
            entry: "cswap".into(),
            kind: EntryKind::Plugin,
            config: serde_json::Value::Null,
            demo: true,
            screen: Some("main".into()),
            size: Size::Wide,
        };

        // cswap is bundled and runs in demo mode, so this works in any build.
        runtime.handle(add("cswap")).await;
        let s = settings.borrow().clone();
        assert_eq!(
            s.config.added,
            Some(AddOutcome {
                module_id: "cswap".into(),
                error: None
            })
        );
        assert_eq!(s.config.error, None);
        assert!(s.modules.iter().any(|m| m.tile_id == "cswap"));
        let tiles = &s.screens[0].tiles;
        assert_eq!(
            (tiles[1].tile_id.as_str(), tiles[1].size),
            ("cswap", Size::Wide)
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with(
                "# mine\nmodules:\n  other: { builtin: cswap, demo: true }  # keep\n  cswap:\n"
            ),
            "{text}"
        );

        // Refused: reported on the outcome, the config is left alone.
        runtime.handle(add("cswap")).await;
        let s = settings.borrow().clone();
        let added = s.config.added.unwrap();
        assert_eq!(added.module_id, "cswap");
        assert!(added.error.unwrap().contains("already in the config"));
        assert_eq!(s.config.error, None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
