//! Shows the pictures in a folder one after another, as an `image` widget.
//! The terminal draws each with the best graphics it supports. Use with
//! `builtin: screensaver`. The module's `refresh:` is how long each picture
//! stays (30 s by default):
//!
//! ```yaml
//! modules:
//!   photos:
//!     builtin: screensaver
//!     refresh: 20s
//!     config: { dir: ~/Pictures/wallpapers }
//! ```

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use dashboard_modules_common::{expand_home, manifest};

/// File types the renderers can decode.
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Folders with more pictures than this are cut, to keep a scan quick.
const MAX_FILES: usize = 10_000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Folder of pictures (PNG, JPEG, GIF, WebP, BMP). `~/` is expanded.
    /// Without it, the module shows the sample pictures built into it.
    #[schemars(extend("x-path" = true))]
    dir: Option<String>,
    /// Show the pictures in a random order. Otherwise by file name.
    shuffle: bool,
    /// Look in subfolders too.
    recursive: bool,
    /// Show the file name under the picture.
    caption: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dir: None,
            shuffle: true,
            recursive: false,
            caption: true,
        }
    }
}

#[derive(Default)]
pub struct Screensaver {
    config: Config,
    dir: PathBuf,
    /// The pictures in the order they are shown.
    order: Vec<PathBuf>,
    /// Index into `order` of the next picture.
    next: usize,
    rng: u64,
}

#[async_trait]
impl Module for Screensaver {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        self.dir = match &self.config.dir {
            Some(dir) => expand_home(dir),
            None => samples()?,
        };
        if !self.dir.is_dir() {
            bail!("{} is not a folder", self.dir.display());
        }
        self.rng = seed();
        let name = self
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Screensaver".into());
        Ok(manifest(
            "screensaver",
            &name,
            30_000,
            2_000,
            Size::Large,
            0,
        ))
    }

    /// Each refresh shows the next picture. The folder is read again at the
    /// end of each round, so added and removed files are picked up.
    async fn render(&mut self) -> Result<View> {
        if self.next >= self.order.len() || !self.order[self.next].is_file() {
            let mut files = images(&self.dir, self.config.recursive)?;
            if self.config.shuffle {
                shuffle(&mut files, &mut self.rng);
            }
            self.order = files;
            self.next = 0;
        }
        let Some(path) = self.order.get(self.next).cloned() else {
            return Ok(empty(&self.dir));
        };
        self.next += 1;
        Ok(view(
            &path,
            &self.dir,
            self.next,
            self.order.len(),
            self.config.caption,
        ))
    }
}

/// The image files in `dir`, by path.
fn images(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries =
            std::fs::read_dir(&folder).with_context(|| format!("reading {}", folder.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if recursive && !hidden(&path) {
                    folders.push(path);
                }
            } else if is_image(&path) && !hidden(&path) {
                out.push(path);
                if out.len() >= MAX_FILES {
                    folders.clear();
                    break;
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

/// Fisher-Yates with xorshift: the order only needs to look random.
fn shuffle(files: &mut [PathBuf], rng: &mut u64) {
    for i in (1..files.len()).rev() {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        files.swap(i, (*rng % (i as u64 + 1)) as usize);
    }
}

fn seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (nanos ^ u64::from(std::process::id())) | 1
}

fn view(path: &Path, dir: &Path, n: usize, total: usize, caption: bool) -> View {
    let name = path
        .strip_prefix(dir)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    View {
        status: Status::Ok,
        summary: format!("{name} · {n} of {total}"),
        root: Widget::Image {
            path: path.to_string_lossy().into_owned(),
            caption: caption.then_some(name),
        },
    }
}

fn empty(dir: &Path) -> View {
    View {
        status: Status::Warn,
        summary: "no pictures".into(),
        root: Widget::Text {
            text: format!(
                "No pictures in {} ({}).",
                dir.display(),
                EXTENSIONS.join(", ")
            ),
            markdown: false,
        },
    }
}

/// One picture in each format the renderers read, about 37 KB in all. They
/// are built in so the demo works with no folder of pictures.
const SAMPLES: &[(&str, &[u8])] = &[
    (
        "ball.gif",
        include_bytes!("../testdata/screensaver/ball.gif"),
    ),
    (
        "hills.jpg",
        include_bytes!("../testdata/screensaver/hills.jpg"),
    ),
    (
        "night.webp",
        include_bytes!("../testdata/screensaver/night.webp"),
    ),
    (
        "peaks.bmp",
        include_bytes!("../testdata/screensaver/peaks.bmp"),
    ),
    (
        "sunset.png",
        include_bytes!("../testdata/screensaver/sunset.png"),
    ),
];

/// Writes the built-in samples to a folder of this user's in the temp
/// folder (once; a file with the wrong size is written again) and returns it.
fn samples() -> Result<PathBuf> {
    #[cfg(unix)]
    // SAFETY: getuid has no preconditions and cannot fail.
    let user = unsafe { libc::getuid() }.to_string();
    #[cfg(not(unix))]
    let user = std::env::var("USERNAME").unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("dashboard-screensaver-samples-{user}"));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for (name, bytes) in SAMPLES {
        let path = dir.join(name);
        if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
            std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(dir)
}

pub fn catalog() -> CatalogEntry {
    // The demo is the sunset sample. If the temp folder can't be written, the
    // renderer says it can't show the picture.
    let dir = samples().unwrap_or_else(|_| std::env::temp_dir());
    let demo = dir.join("sunset.png");
    CatalogEntry {
        description: "Shows the pictures in a folder one after another. The terminal draws \
                      them with Kitty, iTerm2 or Sixel graphics when it supports them, and \
                      with colored half blocks otherwise. Its `refresh:` sets how long each \
                      one stays."
            .into(),
        tags: crate::strings(&["pictures", "local", "files"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry(
            "screensaver",
            "Screensaver",
            Size::Large,
            view(&demo, &dir, 1, SAMPLES.len(), true),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples_in_repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/screensaver")
    }

    fn shown(view: &View) -> String {
        match &view.root {
            Widget::Image { path, .. } => path.clone(),
            other => panic!("not an image: {other:?}"),
        }
    }

    const NAMES: [&str; 5] = [
        "ball.gif",
        "hills.jpg",
        "night.webp",
        "peaks.bmp",
        "sunset.png",
    ];

    #[tokio::test]
    async fn shows_each_picture_in_name_order_then_starts_again() {
        let mut m = Screensaver::default();
        let manifest = m
            .init(serde_json::json!({ "dir": samples_in_repo(), "shuffle": false }))
            .await
            .unwrap();
        assert_eq!(manifest.name, "screensaver");
        let first = m.render().await.unwrap();
        assert_eq!(first.summary, "ball.gif · 1 of 5");
        assert!(matches!(
            first.root,
            Widget::Image { caption: Some(ref c), .. } if c == "ball.gif"
        ));
        for name in &NAMES[1..] {
            assert!(shown(&m.render().await.unwrap()).ends_with(name));
        }
        assert!(shown(&m.render().await.unwrap()).ends_with("ball.gif"));
    }

    #[tokio::test]
    async fn shuffled_rounds_show_every_picture_once() {
        let mut m = Screensaver::default();
        m.init(serde_json::json!({ "dir": samples_in_repo(), "caption": false }))
            .await
            .unwrap();
        let mut seen = Vec::new();
        for _ in NAMES {
            let path = shown(&m.render().await.unwrap());
            seen.push(
                Path::new(&path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        seen.sort();
        assert_eq!(seen, NAMES);
    }

    /// With no folder, the built-in samples are written out and shown.
    #[tokio::test]
    async fn no_folder_shows_the_built_in_samples() {
        let mut m = Screensaver::default();
        m.init(serde_json::json!({ "shuffle": false }))
            .await
            .unwrap();
        let path = shown(&m.render().await.unwrap());
        assert!(path.ends_with("ball.gif"));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(samples_in_repo().join("ball.gif")).unwrap()
        );
    }

    #[tokio::test]
    async fn subfolders_only_when_recursive_and_other_files_are_skipped() {
        let dir = std::env::temp_dir().join(format!("dashboard-saver-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("trip")).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden.png"), "x").unwrap();
        std::fs::write(dir.join("trip/beach.JPG"), "x").unwrap();

        let mut m = Screensaver::default();
        m.init(serde_json::json!({ "dir": dir })).await.unwrap();
        let view = m.render().await.unwrap();
        assert_eq!(view.status, Status::Warn);
        assert_eq!(view.summary, "no pictures");

        let mut m = Screensaver::default();
        m.init(serde_json::json!({ "dir": dir, "recursive": true }))
            .await
            .unwrap();
        let view = m.render().await.unwrap();
        assert_eq!(view.summary, "trip/beach.JPG · 1 of 1");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn config_errors() {
        let err = Screensaver::default()
            .init(serde_json::json!({ "dir": "/no/such/folder" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a folder"));
        assert!(
            Screensaver::default()
                .init(serde_json::json!({ "dir": samples_in_repo(), "intervl": 3 }))
                .await
                .is_err()
        );
    }

    #[test]
    fn demo_is_a_clean_view() {
        let demo = catalog().demo;
        assert!(dashboard_contract::check::check_view(&demo).is_empty());
        assert!(Path::new(&shown(&demo)).is_file());
    }
}
