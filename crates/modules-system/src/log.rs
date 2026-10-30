//! Tails a log file. Use with `builtin: log`:
//!
//! ```yaml
//! modules:
//!   syslog:
//!     builtin: log
//!     config: { path: /var/log/system.log, lines: 300, match: error }
//! ```

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::rpc::LogLevel;
use dashboard_contract::{CatalogEntry, LogLine, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use dashboard_modules_common::{expand_home, manifest};

/// Bytes read from the end of the file per line wanted. Long lines just mean
/// fewer lines shown.
const BYTES_PER_LINE: u64 = 512;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// File to tail. `~/` is expanded.
    path: Option<String>,
    /// Lines to keep.
    lines: usize,
    /// Only lines containing this text (case-insensitive).
    #[serde(rename = "match")]
    matches: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: None,
            lines: 200,
            matches: None,
        }
    }
}

#[derive(Default)]
pub struct Log {
    config: Config,
    path: PathBuf,
}

#[async_trait]
impl Module for Log {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let Some(path) = &self.config.path else {
            bail!("the log module needs `config: {{ path: <file> }}`");
        };
        self.path = expand_home(path);
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Log".into());
        Ok(manifest("log", &name, 1000, 250, Size::Wide, 0))
    }

    async fn render(&mut self) -> Result<View> {
        let text = tail(&self.path, self.config.lines as u64 * BYTES_PER_LINE)
            .with_context(|| format!("reading {}", self.path.display()))?;
        let needle = self.config.matches.as_deref().map(str::to_lowercase);
        let mut lines: Vec<LogLine> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter(|l| {
                needle
                    .as_deref()
                    .is_none_or(|n| l.to_lowercase().contains(n))
            })
            .map(|l| LogLine {
                text: l.to_string(),
                level: guess_level(l),
                time: None,
            })
            .collect();
        let skip = lines.len().saturating_sub(self.config.lines);
        lines.drain(..skip);

        let errors = lines
            .iter()
            .filter(|l| l.level == Some(LogLevel::Error))
            .count();
        let warns = lines
            .iter()
            .filter(|l| l.level == Some(LogLevel::Warn))
            .count();
        let status = if errors > 0 { Status::Warn } else { Status::Ok };
        Ok(View {
            status,
            summary: format!("{} lines · {errors} errors · {warns} warnings", lines.len()),
            root: Widget::Log {
                label: Some(self.path.display().to_string()),
                lines,
            },
        })
    }
}

pub fn catalog() -> CatalogEntry {
    let lines = [
        "2026-10-27 09:14:02 INFO  server listening on :8080",
        "2026-10-27 09:14:05 INFO  connected to postgres (pool 10)",
        "2026-10-27 09:15:41 DEBUG cache miss for /api/users/42",
        "2026-10-27 09:16:10 WARN  slow query: 1.8s SELECT * FROM orders",
        "2026-10-27 09:16:12 INFO  GET /api/orders 200 1834ms",
        "2026-10-27 09:17:30 ERROR payment webhook failed: connection reset",
        "2026-10-27 09:17:31 INFO  retrying payment webhook (1/5)",
        "2026-10-27 09:17:33 INFO  payment webhook delivered",
    ]
    .map(|l| LogLine {
        text: l.into(),
        level: guess_level(l),
        time: None,
    })
    .to_vec();
    let demo = View {
        status: Status::Warn,
        summary: "8 lines · 1 errors · 1 warnings".into(),
        root: Widget::Log {
            label: Some("~/app/logs/server.log".into()),
            lines,
        },
    };
    CatalogEntry {
        description: "Follows a log file, colors lines by level, and can show only lines that \
                      match some text. Add it several times with `builtin: log`."
            .into(),
        tags: crate::strings(&["logs", "local", "files"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("log", "Log", Size::Wide, demo)
    }
}

/// The last `bytes` of a file, starting at a line boundary.
fn tail(path: &PathBuf, bytes: u64) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    // Drop a partial first line, unless we read from the start.
    Ok(match (start > 0, text.find('\n')) {
        (true, Some(i)) => text[i + 1..].to_string(),
        _ => text,
    })
}

/// Level from common markers: "ERROR", "warn:", "[debug]" and so on.
pub fn guess_level(line: &str) -> Option<LogLevel> {
    let l = line.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| l.contains(w));
    if has(&["error", "fatal", "panic", "crit"]) {
        Some(LogLevel::Error)
    } else if has(&["warn"]) {
        Some(LogLevel::Warn)
    } else if has(&["debug", "trace"]) {
        Some(LogLevel::Debug)
    } else if has(&["info"]) {
        Some(LogLevel::Info)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(guess_level("2026 ERROR db down"), Some(LogLevel::Error));
        assert_eq!(guess_level("[warn] slow"), Some(LogLevel::Warn));
        assert_eq!(guess_level("plain line"), None);
    }

    #[tokio::test]
    async fn tails_and_filters() {
        let dir = std::env::temp_dir().join(format!("dashboard-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        let text: String = (0..50)
            .map(|i| format!("{} line {i}\n", if i % 10 == 0 { "ERROR" } else { "INFO" }))
            .collect();
        std::fs::write(&path, text).unwrap();

        let mut m = Log::default();
        m.init(serde_json::json!({ "path": path, "lines": 5 }))
            .await
            .unwrap();
        let view = m.render().await.unwrap();
        let Widget::Log { lines, .. } = view.root else {
            panic!()
        };
        assert_eq!(lines.len(), 5);
        assert_eq!(lines.last().unwrap().text, "INFO line 49");

        let mut m = Log::default();
        m.init(serde_json::json!({ "path": path, "match": "error" }))
            .await
            .unwrap();
        let Widget::Log { lines, .. } = m.render().await.unwrap().root else {
            panic!()
        };
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|l| l.level == Some(LogLevel::Error)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
