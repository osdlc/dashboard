//! Host runtime: runs modules, keeps the latest view per tile, and fans
//! updates out to renderers.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{
    CONTRACT_VERSION, CatalogEntry, LayoutHints, Manifest, Refresh, Status, TileUpdate, View,
    Widget,
};
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio::time::MissedTickBehavior;

pub mod browser;
pub mod history;
mod plugin;
pub mod process;
pub use history::History;
pub use plugin::Plugin;

/// Push mode: each view as it changes, or the error that stopped one. An
/// error is shown like a failed poll: the last good view, marked stale.
pub type ViewSender = mpsc::Sender<Result<View>>;

/// Implemented by built-in modules directly, and by the stdio plugin client.
#[async_trait]
pub trait Module: Send + 'static {
    /// Called once with this module's config block. Returns the manifest.
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest>;
    /// Poll mode: return the current view.
    async fn render(&mut self) -> Result<View>;
    /// Push mode: keep running and send views when they change.
    async fn start(&mut self, _tx: ViewSender) -> Result<()> {
        Ok(())
    }
}

/// Host-side settings for one module, from the config.
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// Overrides the manifest's suggested poll period, clamped to its minimum.
    pub refresh: Option<Duration>,
    /// The module is a [`Demo`]: its tiles are marked as demo data.
    pub demo: bool,
}

/// Stands in for a module whose config says `demo: true`: shows the demo view
/// from its catalog entry once, and runs nothing.
pub struct Demo {
    manifest: Manifest,
    view: View,
}

impl Demo {
    /// The demo for a catalog entry: its name, size and demo view.
    pub fn new(entry: &CatalogEntry) -> Self {
        Self {
            manifest: Manifest {
                id: entry.id.clone(),
                name: entry.name.clone(),
                contract_version: CONTRACT_VERSION.into(),
                refresh: Refresh::Push,
                layout: LayoutHints {
                    size: entry.default_size,
                    priority: 0,
                    min_size: None,
                },
                commands: vec![],
                config_schema: entry.config_schema.clone(),
            },
            view: entry.demo.clone(),
        }
    }

    /// For a module with no catalog entry (a plugin given only as a command):
    /// a tile that says there is no demo view.
    pub fn missing(id: &str) -> Self {
        Self::new(&CatalogEntry {
            id: id.into(),
            name: id.into(),
            version: String::new(),
            description: String::new(),
            tags: vec![],
            kind: dashboard_contract::EntryKind::Plugin,
            source: String::new(),
            default_size: dashboard_contract::Size::Small,
            config_schema: None,
            requirements: Default::default(),
            demo: View {
                status: Status::Unknown,
                summary: "no demo view".into(),
                root: Widget::Text {
                    text: "This plugin is run by command, so it has no catalog entry and no \
                           demo view. Install it as a package to get one."
                        .into(),
                    markdown: false,
                },
            },
        })
    }
}

#[async_trait]
impl Module for Demo {
    async fn init(&mut self, _config: serde_json::Value) -> Result<Manifest> {
        Ok(self.manifest.clone())
    }
    async fn render(&mut self) -> Result<View> {
        Ok(self.view.clone())
    }
    async fn start(&mut self, tx: ViewSender) -> Result<()> {
        // One view; the tile keeps it after this returns.
        let _ = tx.send(Ok(self.view.clone())).await;
        Ok(())
    }
}

/// A running module, for the help and settings screens.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub tile_id: String,
    pub manifest: Manifest,
    /// The poll period actually used; `None` for push modules.
    pub interval: Option<Duration>,
}

/// A tile's latest update, and whether it has ever rendered.
#[derive(Clone)]
struct Tile {
    update: TileUpdate,
    /// False while the tile shows only an error: there is no good view to
    /// keep, so the next error replaces it rather than marking it stale.
    loaded: bool,
}

#[derive(Clone)]
pub struct Host {
    tiles: Arc<RwLock<BTreeMap<String, Tile>>>,
    modules: Arc<RwLock<Vec<ModuleInfo>>>,
    history: Arc<Mutex<History>>,
    /// Background tasks per tile, so a module can be stopped.
    tasks: Arc<Mutex<BTreeMap<String, Vec<tokio::task::JoinHandle<()>>>>>,
    tx: broadcast::Sender<TileUpdate>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tiles: Arc::default(),
            modules: Arc::default(),
            history: Arc::default(),
            tasks: Arc::default(),
            tx,
        }
    }

    /// Initialize a module and start refreshing it in the background.
    /// `tile_id` is the module's key in the config; it names the tile.
    pub async fn spawn(
        &self,
        tile_id: &str,
        mut module: Box<dyn Module>,
        config: serde_json::Value,
        options: SpawnOptions,
    ) -> Result<Manifest> {
        let manifest = module.init(config).await?;
        self.spawn_initialized(tile_id, module, manifest.clone(), options)
            .await;
        Ok(manifest)
    }

    /// Start refreshing a module whose `init` has already returned
    /// `manifest`. Lets a caller init several modules at once and still
    /// start them in a set order.
    pub async fn spawn_initialized(
        &self,
        tile_id: &str,
        mut module: Box<dyn Module>,
        manifest: Manifest,
        options: SpawnOptions,
    ) {
        let tile_id = tile_id.to_string();
        let task_key = tile_id.clone();
        let host = self.clone();
        let demo = options.demo;

        let interval = match manifest.refresh {
            Refresh::Poll {
                interval_ms,
                min_interval_ms,
            } => Some(poll_interval(
                &tile_id,
                interval_ms,
                min_interval_ms,
                options.refresh,
            )),
            Refresh::Push => None,
        };
        self.modules.write().await.push(ModuleInfo {
            tile_id: tile_id.clone(),
            manifest: manifest.clone(),
            interval,
        });

        let handles = match interval {
            Some(interval) => {
                let task = tokio::spawn(async move {
                    let mut tick = tokio::time::interval(interval);
                    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let result = module.render().await;
                        host.publish(&tile_id, result, demo).await;
                    }
                });
                vec![task]
            }
            None => {
                let (vtx, mut vrx) = mpsc::channel(16);
                let log_id = tile_id.clone();
                let run = tokio::spawn(async move {
                    if let Err(err) = module.start(vtx).await {
                        tracing::error!(tile = %log_id, "push module stopped: {err:#}");
                    }
                });
                let forward = tokio::spawn(async move {
                    while let Some(result) = vrx.recv().await {
                        host.publish(&tile_id, result, demo).await;
                    }
                });
                vec![run, forward]
            }
        };
        self.tasks
            .lock()
            .expect("tasks lock")
            .insert(task_key, handles);
    }

    /// Stop a module and forget its tile and history. No-op if not running.
    /// Returns once the module is dropped, so a plugin's processes are gone.
    /// Renderers get the tile's last view once more, marked stale: they keep
    /// the views they were sent, and a module restarted with new settings
    /// (or that fails to restart) must not look current.
    pub async fn stop(&self, tile_id: &str) {
        let handles = self.tasks.lock().expect("tasks lock").remove(tile_id);
        for h in handles.into_iter().flatten() {
            h.abort();
            let _ = h.await;
        }
        self.modules.write().await.retain(|m| m.tile_id != tile_id);
        if let Some(last) = self.tiles.write().await.remove(tile_id) {
            let _ = self.tx.send(TileUpdate {
                stale: true,
                ..last.update
            });
        }
        self.history.lock().expect("history lock").forget(tile_id);
    }

    /// Stop every module. An app that ends the process without dropping its
    /// async runtime calls this first, so plugin processes are killed.
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self
            .tasks
            .lock()
            .expect("tasks lock")
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.stop(&id).await;
        }
    }

    /// Running modules, in spawn order.
    pub async fn modules(&self) -> Vec<ModuleInfo> {
        self.modules.read().await.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TileUpdate> {
        self.tx.subscribe()
    }

    pub async fn snapshot(&self) -> Vec<TileUpdate> {
        self.tiles
            .read()
            .await
            .values()
            .map(|t| t.update.clone())
            .collect()
    }

    async fn publish(&self, tile_id: &str, result: Result<View>, demo: bool) {
        let mut tiles = self.tiles.write().await;
        let tile = match result {
            Ok(mut view) => {
                self.history
                    .lock()
                    .expect("history lock")
                    .apply(tile_id, &mut view);
                Tile {
                    update: TileUpdate {
                        tile_id: tile_id.to_string(),
                        view,
                        updated_at: now_ms(),
                        stale: false,
                        demo,
                    },
                    loaded: true,
                }
            }
            Err(err) => {
                tracing::warn!(tile = %tile_id, "render failed: {err:#}");
                match tiles.get(tile_id) {
                    // Keep the last good view, marked stale.
                    Some(last) if last.loaded => Tile {
                        update: TileUpdate {
                            stale: true,
                            ..last.update.clone()
                        },
                        loaded: true,
                    },
                    // Never loaded: the error is all there is, and it is current.
                    _ => Tile {
                        update: TileUpdate {
                            tile_id: tile_id.to_string(),
                            view: error_view(&err),
                            updated_at: now_ms(),
                            stale: false,
                            demo,
                        },
                        loaded: false,
                    },
                }
            }
        };
        let update = tile.update.clone();
        tiles.insert(tile_id.to_string(), tile);
        // No receivers is fine.
        let _ = self.tx.send(update);
    }
}

/// The config override if given, else the suggestion; never below the minimum.
fn poll_interval(
    tile_id: &str,
    suggested_ms: u64,
    min_ms: Option<u64>,
    over: Option<Duration>,
) -> Duration {
    let wanted = over.unwrap_or(Duration::from_millis(suggested_ms));
    // A zero period would spin; treat 100 ms as the host-wide floor.
    let min = Duration::from_millis(min_ms.unwrap_or(0).max(100));
    if wanted < min {
        tracing::warn!(tile = %tile_id, "refresh {wanted:?} is below the minimum {min:?}; using the minimum");
        min
    } else {
        wanted
    }
}

fn error_view(err: &anyhow::Error) -> View {
    View {
        status: Status::Error,
        summary: format!("error: {err}"),
        root: Widget::Text {
            text: format!("{err:#}"),
            markdown: false,
        },
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_interval_rules() {
        let ms = Duration::from_millis;
        assert_eq!(poll_interval("t", 1000, None, None), ms(1000));
        assert_eq!(
            poll_interval("t", 1000, Some(250), Some(ms(5000))),
            ms(5000)
        );
        assert_eq!(
            poll_interval("t", 60_000, Some(15_000), Some(ms(1000))),
            ms(15_000)
        );
        assert_eq!(poll_interval("t", 0, None, None), ms(100));
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    use dashboard_contract::{CONTRACT_VERSION, LayoutHints, Size};

    struct Counter(u32);

    #[async_trait]
    impl Module for Counter {
        async fn init(&mut self, _: serde_json::Value) -> Result<Manifest> {
            Ok(Manifest {
                id: "counter".into(),
                name: "Counter".into(),
                contract_version: CONTRACT_VERSION.into(),
                refresh: Refresh::Poll {
                    interval_ms: 100,
                    min_interval_ms: None,
                },
                layout: LayoutHints {
                    size: Size::Small,
                    priority: 0,
                    min_size: None,
                },
                commands: vec![],
                config_schema: None,
            })
        }
        async fn render(&mut self) -> Result<View> {
            self.0 += 1;
            Ok(View {
                status: Status::Ok,
                summary: self.0.to_string(),
                root: Widget::Text {
                    text: String::new(),
                    markdown: false,
                },
            })
        }
    }

    /// A tile that never rendered shows its error as current, not stale;
    /// once it has rendered, an error keeps that view, marked stale.
    #[tokio::test]
    async fn an_error_is_stale_only_after_a_good_view() {
        let host = Host::new();
        let view = |s: &str| View {
            status: Status::Ok,
            summary: s.into(),
            root: Widget::Text {
                text: s.into(),
                markdown: false,
            },
        };
        let tile = || async { host.snapshot().await.remove(0) };
        host.publish("t", Err(anyhow::anyhow!("no token")), false)
            .await;
        let t = tile().await;
        assert!(!t.stale);
        assert_eq!(t.view.summary, "error: no token");
        host.publish("t", Err(anyhow::anyhow!("still no token")), false)
            .await;
        let t = tile().await;
        assert!(!t.stale);
        assert_eq!(t.view.summary, "error: still no token");

        host.publish("t", Ok(view("fine")), false).await;
        assert!(!tile().await.stale);
        host.publish("t", Err(anyhow::anyhow!("offline")), false)
            .await;
        let t = tile().await;
        assert!(t.stale);
        assert_eq!(t.view.summary, "fine");
    }

    #[tokio::test]
    async fn stop_removes_module_and_tile() {
        let host = Host::new();
        host.spawn(
            "c",
            Box::new(Counter(0)),
            serde_json::Value::Null,
            SpawnOptions::default(),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(host.modules().await.len(), 1);
        assert_eq!(host.snapshot().await.len(), 1);

        let mut updates = host.subscribe();
        host.stop("c").await;
        // Renderers are told the view they hold is no longer current.
        let mut last = updates.recv().await.unwrap();
        while let Ok(next) = updates.try_recv() {
            last = next;
        }
        assert_eq!(last.tile_id, "c");
        assert!(last.stale);
        assert!(host.modules().await.is_empty());
        assert!(host.snapshot().await.is_empty());
        // The task is gone, so nothing is published again.
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(host.snapshot().await.is_empty());
    }

    /// A counter that notes when it is dropped.
    struct Tracked(Counter, Arc<std::sync::atomic::AtomicBool>);

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.1.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl Module for Tracked {
        async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
            self.0.init(config).await
        }
        async fn render(&mut self) -> Result<View> {
            self.0.render().await
        }
    }

    /// `stop` and `stop_all` return once the modules are dropped, so what
    /// they hold (a plugin's processes) is gone.
    #[tokio::test]
    async fn stop_all_drops_the_modules() {
        let host = Host::new();
        let dropped: Vec<_> = (0..2).map(|_| Arc::default()).collect();
        for (i, flag) in dropped.iter().enumerate() {
            let module = Tracked(Counter(0), Arc::clone(flag));
            host.spawn(
                &format!("t{i}"),
                Box::new(module),
                serde_json::Value::Null,
                SpawnOptions::default(),
            )
            .await
            .unwrap();
        }
        host.stop("t0").await;
        assert!(dropped[0].load(std::sync::atomic::Ordering::SeqCst));
        host.stop_all().await;
        assert!(dropped[1].load(std::sync::atomic::Ordering::SeqCst));
        assert!(host.modules().await.is_empty());
    }

    #[tokio::test]
    async fn demo_shows_its_view_marked_as_demo() {
        let host = Host::new();
        let manifest = host
            .spawn(
                "d",
                Box::new(Demo::missing("d")),
                serde_json::Value::Null,
                SpawnOptions {
                    demo: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(manifest.refresh, Refresh::Push);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let tiles = host.snapshot().await;
        assert_eq!(tiles.len(), 1);
        assert!(tiles[0].demo);
        assert_eq!(tiles[0].view.summary, "no demo view");
    }
}
