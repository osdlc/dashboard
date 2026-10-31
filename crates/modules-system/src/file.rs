//! Shows a file that something else writes: a view or widget in the
//! contract's JSON, values picked out of any JSON, or text. Watches the file
//! and shows it again when it changes; where watching can't start, checks it
//! every second instead. Add it several times with `builtin: file`:
//!
//! ```yaml
//! modules:
//!   backups:
//!     builtin: file
//!     config:
//!       path: ~/.cache/backup/status.json   # written by a cron job
//!   build:
//!     builtin: file
//!     config:
//!       path: ./build/stats.json
//!       fields:                              # pick values out of any JSON
//!         - { label: Warnings, pointer: /warnings }
//!         - { label: Size, pointer: /bundle/kb, unit: KB }
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Refresh, Size, View};
use dashboard_host::{Module, ViewSender};
use dashboard_modules_common::{expand_home, manifest};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::feed::{self, Field, Format, MAX_BYTES, Shape};

/// How often the file is checked when it can't be watched.
const POLL: Duration = Duration::from_secs(1);
/// While watching, the file is still checked this often, in case a change
/// was missed (some network file systems send no events).
const RECHECK: Duration = Duration::from_secs(30);
/// Events that come close together (an editor's save, a writer's several
/// writes) cause one read.
const SETTLE: Duration = Duration::from_millis(100);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// File to show. `~/` is expanded; a relative path starts from the
    /// config file's folder.
    #[schemars(extend("x-path" = true))]
    path: Option<String>,
    /// Tile title. Default: the file name.
    name: Option<String>,
    /// How to read the file.
    format: Format,
    /// Values to pick out of a JSON file, each shown as a stat.
    fields: Vec<Field>,
    /// Render text as markdown. Default: true for `.md` and `.markdown` files.
    markdown: Option<bool>,
    /// Lines of text shown, from the top.
    lines: usize,
    /// Watch the file for changes. With `false` it is checked on each
    /// refresh instead (every second, or the module's `refresh:`).
    watch: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: None,
            name: None,
            format: Format::Auto,
            fields: vec![],
            markdown: None,
            lines: 200,
            watch: true,
        }
    }
}

#[derive(Default)]
pub struct File {
    config: Config,
    path: PathBuf,
    name: String,
    markdown: bool,
    /// Modified time and size the last view was built from.
    seen: Option<(SystemTime, u64)>,
    last: Option<View>,
}

#[async_trait]
impl Module for File {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let Some(path) = &self.config.path else {
            bail!("the file module needs `config: {{ path: <file> }}`");
        };
        feed::check(self.config.format, &self.config.fields, self.config.lines)?;
        self.path = expand_home(path);
        self.name = self
            .config
            .name
            .clone()
            .unwrap_or_else(|| file_name(&self.path));
        self.markdown = self
            .config
            .markdown
            .unwrap_or_else(|| is_markdown(&self.path));
        let mut m = manifest("file", &self.name, 1000, 250, Size::Medium, 0);
        if self.config.watch {
            m.refresh = Refresh::Push;
        }
        Ok(m)
    }

    /// Reads the file again only when its modified time or size changed.
    async fn render(&mut self) -> Result<View> {
        let meta = std::fs::metadata(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        let key = (meta.modified()?, meta.len());
        if let (Some(seen), Some(last)) = (self.seen, &self.last)
            && seen == key
        {
            return Ok(last.clone());
        }
        if meta.len() > MAX_BYTES {
            bail!("{} is over 1 MB", self.path.display());
        }
        let bytes = std::fs::read(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        let shape = Shape {
            format: self.config.format,
            fields: &self.config.fields,
            markdown: self.markdown,
            lines: self.config.lines,
        };
        let view = feed::view(&String::from_utf8_lossy(&bytes), &self.name, &shape)
            .with_context(|| format!("reading {}", self.path.display()))?;
        self.seen = Some(key);
        self.last = Some(view.clone());
        Ok(view)
    }

    /// Watch mode: sends a view, then waits for the file to change. Sends
    /// again only when the view or the error is different.
    async fn start(&mut self, tx: ViewSender) -> Result<()> {
        let (etx, mut erx) = mpsc::channel(1);
        // Kept until this returns: dropping it stops the watching.
        let mut watch = Watch::new(&self.path, etx);
        let wait = match &watch {
            Ok(_) => RECHECK,
            Err(err) => {
                tracing::warn!(
                    path = %self.path.display(),
                    "can't watch the file, checking it every second: {err:#}"
                );
                POLL
            }
        };
        let mut sent: Option<Result<View, String>> = None;
        loop {
            let result = self.render().await;
            let now = result
                .as_ref()
                .map(View::clone)
                .map_err(|e| format!("{e:#}"));
            if sent.as_ref() != Some(&now) {
                sent = Some(now);
                if tx.send(result).await.is_err() {
                    return Ok(());
                }
            }
            tokio::select! {
                Some(()) = erx.recv() => {
                    tokio::time::sleep(SETTLE).await;
                    while erx.try_recv().is_ok() {}
                    if let Ok(watch) = &mut watch {
                        watch.file();
                    }
                }
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }
}

/// Watches the file's folder, so a file that is replaced (as editors and
/// careful writers do), removed or created later is seen, and the file
/// itself, since some systems (kqueue on macOS) report writes to a file only
/// to a watch on that file. Any event sends on `tx`: a read is cheap when
/// the file has not changed.
struct Watch {
    watcher: notify::RecommendedWatcher,
    path: PathBuf,
}

impl Watch {
    fn new(path: &Path, tx: mpsc::Sender<()>) -> Result<Self> {
        use notify::Watcher;
        let name = path.file_name().context("the path has no file name")?;
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        // The real path, so events name it as watched (/var is /private/var
        // on macOS).
        let dir = dir.canonicalize().unwrap_or(dir);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok() {
                    // Full means a read is already due.
                    let _ = tx.try_send(());
                }
            })?;
        watcher
            .watch(&dir, notify::RecursiveMode::NonRecursive)
            .with_context(|| format!("watching {}", dir.display()))?;
        let mut watch = Self {
            watcher,
            path: dir.join(name),
        };
        watch.file();
        Ok(watch)
    }

    /// (Re)watches the file itself: done after each event, since a replaced
    /// file is a new file. Missing for now is fine; the folder watch sees it
    /// come back.
    fn file(&mut self) {
        use notify::Watcher;
        let _ = self.watcher.unwatch(&self.path);
        let _ = self
            .watcher
            .watch(&self.path, notify::RecursiveMode::NonRecursive);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "File".into())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

/// The demo is the view in the `backup.json` sample.
const DEMO: &str = include_str!("../testdata/feeds/backup.json");

pub fn catalog() -> CatalogEntry {
    let shape = Shape {
        format: Format::Auto,
        fields: &[],
        markdown: false,
        lines: 200,
    };
    CatalogEntry {
        description: "Shows a file another program writes: a view or widget in the \
                      dashboard's JSON, values picked out of any JSON, or text. Watches \
                      the file and shows it again when it changes. Add it several times \
                      with `builtin: file`."
            .into(),
        tags: crate::strings(&["files", "local", "json", "custom"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry(
            "file",
            "File",
            Size::Medium,
            feed::view(DEMO, "backup.json", &shape).expect("the demo sample is a view"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{Status, Widget};

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("dashboard-file-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn summary(r: &Result<View>) -> String {
        match r {
            Ok(v) => v.summary.clone(),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tokio::test]
    async fn polls_when_watch_is_off() {
        let dir = Dir::new("poll");
        let path = dir.0.join("status.json");
        std::fs::write(&path, r#"{"type":"stat","label":"Jobs","value":"3"}"#).unwrap();
        let mut m = File::default();
        let manifest = m
            .init(serde_json::json!({ "path": path, "watch": false }))
            .await
            .unwrap();
        assert!(matches!(manifest.refresh, Refresh::Poll { .. }));
        assert_eq!(manifest.name, "status.json");
        let v = m.render().await.unwrap();
        assert!(matches!(v.root, Widget::Stat { .. }));
        std::fs::write(&path, "plain text\n").unwrap();
        assert_eq!(m.render().await.unwrap().summary, "status.json · 1 line");
        std::fs::remove_file(&path).unwrap();
        assert!(m.render().await.is_err());
    }

    async fn next(rx: &mut mpsc::Receiver<Result<View>>) -> String {
        let got = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await;
        summary(&got.expect("no update in 10s").unwrap())
    }

    /// Watch mode sends a view when the file changes, an error when it goes
    /// away, and a view again when it comes back.
    #[tokio::test(flavor = "multi_thread")]
    async fn watches_the_file() {
        let dir = Dir::new("watch");
        let path = dir.0.join("notes.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut m = File::default();
        let manifest = m.init(serde_json::json!({ "path": path })).await.unwrap();
        assert_eq!(manifest.refresh, Refresh::Push);
        let (tx, mut rx) = mpsc::channel(8);
        let task = tokio::spawn(async move { m.start(tx).await });

        assert_eq!(next(&mut rx).await, "notes.txt · 1 line");
        // Let the watcher start before the first change.
        tokio::time::sleep(Duration::from_millis(300)).await;
        std::fs::write(&path, "one\ntwo\n").unwrap();
        assert_eq!(next(&mut rx).await, "notes.txt · 2 lines");
        std::fs::remove_file(&path).unwrap();
        assert!(next(&mut rx).await.starts_with("error: reading"));
        std::fs::write(&path, "back\n").unwrap();
        assert_eq!(next(&mut rx).await, "notes.txt · 1 line");
        task.abort();
    }

    #[tokio::test]
    async fn picks_fields() {
        let dir = Dir::new("fields");
        let path = dir.0.join("build.json");
        std::fs::write(&path, r#"{"warnings":4,"bundle":{"kb":512.5}}"#).unwrap();
        let mut m = File::default();
        m.init(serde_json::json!({
            "path": path,
            "name": "Build",
            "watch": false,
            "fields": [
                { "label": "Warnings", "pointer": "/warnings" },
                { "label": "Size", "pointer": "/bundle/kb", "unit": "KB" },
            ],
        }))
        .await
        .unwrap();
        let v = m.render().await.unwrap();
        assert_eq!(v.status, Status::Ok);
        assert_eq!(v.summary, "Warnings 4 · Size 512.5 KB");
    }

    #[tokio::test]
    async fn config_errors() {
        for config in [
            serde_json::json!({}),
            serde_json::json!({ "path": "x", "lines": 0 }),
            serde_json::json!({ "path": "x", "pth": "y" }),
            serde_json::json!({ "path": "x", "format": "xml" }),
            serde_json::json!({ "path": "x", "fields": [{ "label": "a", "pointer": "a" }] }),
        ] {
            assert!(
                File::default().init(config.clone()).await.is_err(),
                "{config}"
            );
        }
    }

    #[test]
    fn demo_is_valid() {
        let problems: Vec<String> = dashboard_contract::check::check_view(&catalog().demo)
            .into_iter()
            .filter(|f| f.problem)
            .map(|f| f.text)
            .collect();
        assert!(problems.is_empty(), "{problems:?}");
    }
}
