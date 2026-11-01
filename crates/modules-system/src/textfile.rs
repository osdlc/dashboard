//! Shows a text or markdown file, and shows it again when it changes. Use
//! with `builtin: textfile`:
//!
//! ```yaml
//! modules:
//!   todo:
//!     builtin: textfile
//!     config: { path: ~/notes/todo.md }
//! ```
//!
//! A native-vs-plugin benchmark widget: the TypeScript and Python plugins in
//! `plugins/ext/textfile-ts` and `plugins/ext/textfile-py` follow the same
//! rules and show the same view for the same file. The shared samples and
//! expected views are in `testdata/textfile`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use dashboard_modules_common::{expand_home, manifest};

/// Larger files are refused: this is for notes, not logs (use `log` for those).
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// File to show. `~/` is expanded.
    path: Option<String>,
    /// Render as markdown. Default: true for `.md` and `.markdown` files.
    markdown: Option<bool>,
    /// Lines to show, from the top.
    lines: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: None,
            markdown: None,
            lines: 200,
        }
    }
}

#[derive(Default)]
pub struct TextFile {
    config: Config,
    path: PathBuf,
    name: String,
    markdown: bool,
    /// Modified time and size the last view was built from.
    seen: Option<(SystemTime, u64)>,
    last: Option<View>,
}

#[async_trait]
impl Module for TextFile {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let Some(path) = &self.config.path else {
            bail!("the textfile module needs `config: {{ path: <file> }}`");
        };
        if self.config.lines == 0 {
            bail!("`lines` must be at least 1");
        }
        self.path = expand_home(path);
        self.name = file_name(&self.path);
        self.markdown = self
            .config
            .markdown
            .unwrap_or_else(|| is_markdown(&self.path));
        Ok(manifest("textfile", &self.name, 1000, 250, Size::Tall, 0))
    }

    /// Checks the file's modified time and size each second, and reads it
    /// again only when one of them changed.
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
        let view = view(
            &String::from_utf8_lossy(&bytes),
            &self.name,
            self.markdown,
            self.config.lines,
        );
        self.seen = Some(key);
        self.last = Some(view.clone());
        Ok(view)
    }
}

/// The tile for a file's text. All three versions of the widget follow these
/// rules: drop a leading byte order mark, turn CRLF into LF, drop trailing
/// blanks (spaces, tabs, line breaks), and keep the first `max_lines` lines.
pub fn view(text: &str, name: &str, markdown: bool, max_lines: usize) -> View {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n");
    let text = text.trim_end_matches([' ', '\t', '\r', '\n']);
    if text.is_empty() {
        return View {
            status: Status::Ok,
            summary: format!("{name} · empty"),
            root: Widget::Text {
                text: "(empty)".into(),
                markdown: false,
            },
        };
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let (shown, summary) = if lines.len() > max_lines {
        (
            format!("{}\n…", lines[..max_lines].join("\n")),
            format!("{name} · first {max_lines} of {} lines", lines.len()),
        )
    } else {
        let count = match lines.len() {
            1 => "1 line".to_string(),
            n => format!("{n} lines"),
        };
        (text.to_string(), format!("{name} · {count}"))
    };
    View {
        status: Status::Ok,
        summary,
        root: Widget::Text {
            text: shown,
            markdown,
        },
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Text file".into())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

/// The demo is the view of the `notes.md` sample.
const DEMO: &str = include_str!("../testdata/textfile/notes.md");

pub fn catalog() -> CatalogEntry {
    CatalogEntry {
        description: "Shows a text or markdown file and shows it again when it changes. \
                      Good for a todo list or notes. Add it several times with \
                      `builtin: textfile`."
            .into(),
        tags: crate::strings(&["notes", "local", "files"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry(
            "textfile",
            "Text file",
            Size::Tall,
            view(DEMO, "notes.md", true, 200),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/textfile")
            .join(name)
    }

    /// Each case in `testdata/textfile/cases.json` gives the expected view.
    /// The plugin host tests check the TypeScript and Python plugins against
    /// the same list.
    #[tokio::test]
    async fn samples_match_the_expected_views() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(data("cases.json")).unwrap()).unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let file = data(case["file"].as_str().unwrap());
            let mut config = case["config"].clone();
            config["path"] = file.to_string_lossy().into();
            let mut m = TextFile::default();
            let manifest = m.init(config).await.unwrap();
            assert_eq!(manifest.name, file_name(&file));
            let got = m.render().await.unwrap();
            let text = std::fs::read_to_string(data(case["view"].as_str().unwrap())).unwrap();
            let expected: View = serde_json::from_str(&text).unwrap();
            assert_eq!(got, expected, "{case}");
        }
    }

    #[test]
    fn demo_is_the_notes_sample() {
        let text = std::fs::read_to_string(data("notes.md.view.json")).unwrap();
        let expected: View = serde_json::from_str(&text).unwrap();
        assert_eq!(catalog().demo, expected);
    }

    #[tokio::test]
    async fn shows_the_file_again_when_it_changes() {
        let dir =
            std::env::temp_dir().join(format!("dashboard-textfile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("todo.txt");
        std::fs::write(&path, "one\n").unwrap();

        let mut m = TextFile::default();
        m.init(serde_json::json!({ "path": path })).await.unwrap();
        assert_eq!(m.render().await.unwrap().summary, "todo.txt · 1 line");
        // A different size counts as a change even within the same mtime tick.
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let view = m.render().await.unwrap();
        assert_eq!(view.summary, "todo.txt · 2 lines");
        assert_eq!(
            view.root,
            Widget::Text {
                text: "one\ntwo".into(),
                markdown: false
            }
        );

        std::fs::remove_file(&path).unwrap();
        assert!(m.render().await.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn config_errors() {
        assert!(
            TextFile::default()
                .init(serde_json::json!({}))
                .await
                .is_err()
        );
        let err = TextFile::default()
            .init(serde_json::json!({ "path": "x", "lines": 0 }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("lines"));
        assert!(
            TextFile::default()
                .init(serde_json::json!({ "path": "x", "pth": "y" }))
                .await
                .is_err()
        );
    }
}
