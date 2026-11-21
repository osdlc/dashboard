//! Shows the pictures in a folder one after another. Renders as a
//! [`Widget::Image`]. Use with `builtin: screensaver`. The module's `refresh:` is how long each picture
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
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use dashboard_modules_common::{expand_home, manifest};

/// File types the renderers can decode. Keep in step with the image crate
/// features in crates/tui/Cargo.toml and the MIME match in crates/web/src/lib.rs.
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Folders with more pictures than this are cut, to keep a scan quick. Which
/// ones are kept then depends on the order the system lists them in.
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

/// Shows the pictures in a folder one after another, as an `image` widget.
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
        // The samples' folder name means nothing to the user.
        let name = self
            .config
            .dir
            .as_ref()
            .and_then(|_| self.dir.file_name())
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

    /// Each refresh shows the next picture; files removed since the folder was
    /// read are skipped. The folder is read again at the end of each round, so
    /// added and removed files are picked up.
    async fn render(&mut self) -> Result<View> {
        while self.next < self.order.len() && !self.order[self.next].is_file() {
            self.next += 1;
        }
        if self.next >= self.order.len() {
            let last = self.order.last().cloned();
            let mut files = images(&self.dir, self.config.recursive)?;
            if self.config.shuffle {
                shuffle(&mut files, &mut self.rng);
                // A new round doesn't start with the picture that ended the last.
                if files.len() > 1 && files.first() == last.as_ref() {
                    let end = files.len() - 1;
                    files.swap(0, end);
                }
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

/// The image files in `dir`, sorted by path, without hidden entries; at most
/// [`MAX_FILES`]. A subfolder that can't be read is skipped.
fn images(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(_) if folder != dir => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {}", folder.display())),
        };
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
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
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

/// One picture in each format the renderers read, built in so the demo works
/// with no folder of pictures.
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
/// folder and returns it. A file that is missing or has the wrong size is
/// written again, under a temporary name and then renamed: a reader never
/// sees half a file, and a link planted in its place is replaced, not followed.
fn samples() -> Result<PathBuf> {
    static WRITES: AtomicUsize = AtomicUsize::new(0);
    #[cfg(unix)]
    let user = rustix::process::getuid().as_raw();
    #[cfg(not(unix))]
    let user = std::env::var("USERNAME").unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("dashboard-screensaver-samples-{user}"));
    #[cfg(unix)]
    private_dir(&dir, user)?;
    #[cfg(not(unix))]
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for (name, bytes) in SAMPLES {
        let path = dir.join(name);
        let len = std::fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
        if len != Some(bytes.len() as u64) {
            let n = WRITES.fetch_add(1, Ordering::Relaxed);
            let temp = dir.join(format!(".{name}.{}.{n}", std::process::id()));
            std::fs::write(&temp, bytes)
                .and_then(|()| std::fs::rename(&temp, &path))
                .with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(dir)
}

/// Makes `dir` a folder of `user`'s that only they can open. The temp folder
/// is shared, so a folder or link there that someone else made is refused.
#[cfg(unix)]
fn private_dir(dir: &Path, user: u32) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
            return Err(e).with_context(|| format!("creating {}", dir.display()));
        }
        _ => {}
    }
    let m = std::fs::symlink_metadata(dir).with_context(|| format!("reading {}", dir.display()))?;
    if !m.is_dir() || m.uid() != user {
        bail!("{} is not a folder of this user's", dir.display());
    }
    // Made with the umask's mode before this check existed.
    if m.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("securing {}", dir.display()))?;
    }
    Ok(())
}

/// Catalog entry for this module, with demo data.
pub fn catalog() -> CatalogEntry {
    // The catalog is built often, so the samples are written once per run.
    // If the temp folder can't be written, the renderer says it can't show
    // the picture.
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| samples().unwrap_or_else(|_| std::env::temp_dir()));
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
            view(&demo, dir, 1, SAMPLES.len(), true),
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

    fn names() -> Vec<&'static str> {
        SAMPLES.iter().map(|(n, _)| *n).collect()
    }

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
        for name in &names()[1..] {
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
        for _ in SAMPLES {
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
        assert_eq!(seen, names());
    }

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
        let name = Path::new("trip").join("beach.JPG");
        assert_eq!(view.summary, format!("{} · 1 of 1", name.display()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_removed_file_is_skipped_and_rounds_do_not_repeat() {
        let dir = std::env::temp_dir().join(format!("dashboard-saver-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.png", "b.png", "c.png"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let mut m = Screensaver::default();
        m.init(serde_json::json!({ "dir": dir, "shuffle": false }))
            .await
            .unwrap();
        assert!(shown(&m.render().await.unwrap()).ends_with("a.png"));
        std::fs::remove_file(dir.join("b.png")).unwrap();
        assert!(shown(&m.render().await.unwrap()).ends_with("c.png"));
        assert!(shown(&m.render().await.unwrap()).ends_with("a.png"));

        let mut m = Screensaver::default();
        m.init(serde_json::json!({ "dir": dir })).await.unwrap();
        let mut last = String::new();
        for _ in 0..20 {
            let path = shown(&m.render().await.unwrap());
            assert_ne!(path, last);
            last = path;
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_samples_folder_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let user = rustix::process::getuid().as_raw();
        let dir =
            std::env::temp_dir().join(format!("dashboard-saver-private-{}", std::process::id()));
        let mode = |d: &Path| std::fs::metadata(d).unwrap().permissions().mode() & 0o777;
        private_dir(&dir, user).unwrap();
        assert_eq!(mode(&dir), 0o700);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        private_dir(&dir, user).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert!(private_dir(&dir, user.wrapping_add(1)).is_err());
        let link = dir.with_extension("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(private_dir(&link, user).is_err());
        std::fs::remove_file(&link).unwrap();
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
