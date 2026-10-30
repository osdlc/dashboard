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

pub mod history;
mod plugin;
pub use history::History;
pub use plugin::Plugin;

pub type ViewSender = mpsc::Sender<View>;

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
        let _ = tx.send(self.view.clone()).await;
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

#[derive(Clone)]
pub struct Host {
    tiles: Arc<RwLock<BTreeMap<String, TileUpdate>>>,
    modules: Arc<RwLock<Vec<ModuleInfo>>>,
    history: Arc<Mutex<History>>,
    /// Background tasks per tile, so a module can be stopped.
    tasks: Arc<Mutex<BTreeMap<String, Vec<tokio::task::AbortHandle>>>>,
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
                vec![task.abort_handle()]
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
                    while let Some(view) = vrx.recv().await {
                        host.publish(&tile_id, Ok(view), demo).await;
                    }
                });
                vec![run.abort_handle(), forward.abort_handle()]
            }
        };
        self.tasks
            .lock()
            .expect("tasks lock")
            .insert(task_key, handles);
        Ok(manifest)
    }

    /// Stop a module and forget its tile and history. No-op if not running.
    pub async fn stop(&self, tile_id: &str) {
        let handles = self.tasks.lock().expect("tasks lock").remove(tile_id);
        for h in handles.into_iter().flatten() {
            h.abort();
        }
        self.modules.write().await.retain(|m| m.tile_id != tile_id);
        self.tiles.write().await.remove(tile_id);
        self.history.lock().expect("history lock").forget(tile_id);
    }

    /// Running modules, in spawn order.
    pub async fn modules(&self) -> Vec<ModuleInfo> {
        self.modules.read().await.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TileUpdate> {
        self.tx.subscribe()
    }

    pub async fn snapshot(&self) -> Vec<TileUpdate> {
        self.tiles.read().await.values().cloned().collect()
    }

    async fn publish(&self, tile_id: &str, result: Result<View>, demo: bool) {
        let mut tiles = self.tiles.write().await;
        let update = match result {
            Ok(mut view) => {
                self.history
                    .lock()
                    .expect("history lock")
                    .apply(tile_id, &mut view);
                TileUpdate {
                    tile_id: tile_id.to_string(),
                    view,
                    updated_at: now_ms(),
                    stale: false,
                    demo,
                }
            }
            Err(err) => {
                tracing::warn!(tile = %tile_id, "render failed: {err:#}");
                match tiles.get(tile_id) {
                    // Keep the last good view, marked stale.
                    Some(last) => TileUpdate {
                        stale: true,
                        ..last.clone()
                    },
                    None => TileUpdate {
                        tile_id: tile_id.to_string(),
                        view: error_view(&err),
                        updated_at: now_ms(),
                        stale: true,
                        demo,
                    },
                }
            }
        };
        tiles.insert(tile_id.to_string(), update.clone());
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

        host.stop("c").await;
        assert!(host.modules().await.is_empty());
        assert!(host.snapshot().await.is_empty());
        // The task is gone, so nothing is published again.
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(host.snapshot().await.is_empty());
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
