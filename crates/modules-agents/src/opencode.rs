//! OpenCode token use and cost, from its local data. Current OpenCode keeps
//! messages in a SQLite database (`~/.local/share/opencode/opencode.db`, or
//! `opencode-<channel>.db`), read with the `sqlite3` CLI. Older versions wrote
//! one JSON file per message (`storage/message/<session>/<id>.json`); both are
//! read. Assistant messages carry the tokens and cost of the reply. No network.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

use super::now_secs;
use super::tally::{DAYS, Record, Tally, recent_files};
use dashboard_modules_common::{expand_home, manifest};

const DOCS: &str = "https://opencode.ai/docs";

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// OpenCode's data folder; default `$XDG_DATA_HOME/opencode` or
    /// `~/.local/share/opencode`.
    #[schemars(extend("x-path" = true))]
    home: Option<String>,
}

#[derive(Default)]
pub struct OpenCode {
    home: PathBuf,
}

#[async_trait]
impl Module for OpenCode {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        let config: Config = if config.is_null() {
            Config::default()
        } else {
            serde_json::from_value(config)?
        };
        self.home = match config.home {
            Some(h) => expand_home(&h),
            None => match std::env::var("XDG_DATA_HOME") {
                Ok(d) if !d.is_empty() => PathBuf::from(d).join("opencode"),
                _ => expand_home("~/.local/share/opencode"),
            },
        };
        Ok(manifest(
            "opencode",
            "OpenCode",
            120_000,
            15_000,
            Size::Medium,
            46,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let now = now_secs();
        let mut records = read_messages(&self.home, now)?;
        for db in databases(&self.home) {
            records.extend(read_database(&db, now).await?);
        }
        Ok(view(&Tally::new(&records, now), now))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    role: String,
    time: Time,
    #[serde(rename = "modelID", default)]
    model_id: Option<String>,
    #[serde(default)]
    cost: f64,
    #[serde(default)]
    tokens: Option<Tokens>,
}

#[derive(Debug, Deserialize)]
struct Time {
    /// Milliseconds since the epoch.
    created: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Tokens {
    input: u64,
    output: u64,
    reasoning: u64,
    cache: Cache,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Cache {
    read: u64,
    write: u64,
}

fn read_messages(home: &Path, now: i64) -> Result<Vec<Record>> {
    if !home.is_dir() {
        bail!(
            "no OpenCode data in {} (is OpenCode installed?)",
            home.display()
        );
    }
    // Installed but never used: no message folder yet, so nothing to count.
    let dir = home.join("storage/message");
    Ok(recent_files(&dir, &["json"], now)
        .into_iter()
        .filter_map(|f| parse(&std::fs::read_to_string(f).ok()?))
        .collect())
}

/// OpenCode's databases in its data folder: `opencode.db` and one per
/// release channel (`opencode-beta.db`, ...).
fn databases(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return vec![];
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("opencode") && n.ends_with(".db"))
        })
        .collect();
    out.sort();
    out
}

/// The last week's messages from one database, opened read-only.
async fn read_database(db: &Path, now: i64) -> Result<Vec<Record>> {
    let since_ms = (now - DAYS * 86_400) * 1000;
    let out = Command::new("sqlite3")
        .arg("-readonly")
        .arg("-json")
        .arg(db)
        .arg(format!(
            "SELECT data FROM message WHERE time_created >= {since_ms}"
        ))
        .stdin(Stdio::null())
        .output()
        .await
        .context("running sqlite3 (needed to read OpenCode's database)")?;
    if !out.status.success() {
        bail!(
            "sqlite3 could not read the OpenCode database {}: {}",
            db.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(parse_rows(&String::from_utf8_lossy(&out.stdout)))
}

/// `sqlite3 -json` output: `[{"data":"<message JSON>"}, ...]`, or nothing
/// when no rows match.
fn parse_rows(text: &str) -> Vec<Record> {
    #[derive(Deserialize)]
    struct Row {
        data: String,
    }
    if text.trim().is_empty() {
        return vec![];
    }
    serde_json::from_str::<Vec<Row>>(text)
        .unwrap_or_default()
        .iter()
        .filter_map(|r| parse(&r.data))
        .collect()
}

/// One message (a file, or a database row's data): a record for an assistant reply, None otherwise.
fn parse(text: &str) -> Option<Record> {
    let m: Message = serde_json::from_str(text).ok()?;
    if m.role != "assistant" {
        return None;
    }
    let t = m.tokens.unwrap_or_default();
    Some(Record {
        at: m.time.created / 1000,
        tokens: t.input + t.output + t.reasoning + t.cache.read + t.cache.write,
        cost: m.cost,
        model: m.model_id.unwrap_or_else(|| "unknown".into()),
    })
}

fn view(t: &Tally, now: i64) -> View {
    View {
        status: t.status(),
        summary: t.summary(),
        root: Widget::Column {
            children: [
                t.widgets(now),
                vec![super::link("OpenCode docs", "opencode.ai", DOCS)],
            ]
            .concat(),
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let now = now_secs();
    let records =
        super::tally::demo_records(now, &[("claude-sonnet-4-5", 0.7), ("gpt-5", 0.3)], true);
    CatalogEntry {
        description: "OpenCode tokens and cost per day, and the models used, from OpenCode's \
                      local database (read with sqlite3) or older message files. No network."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "local"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["sqlite3"]),
            ..Requirements::default()
        },
        ..crate::entry(
            "opencode",
            "OpenCode",
            Size::Medium,
            view(&Tally::new(&records, now), now),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_modules_common::time::parse_time;

    // Shaped like OpenCode's message files, with ids shortened.
    const REPLY: &str = r#"{"id":"msg_01","sessionID":"ses_01","role":"assistant","time":{"created":1793095200000,"completed":1793095212000},"modelID":"claude-sonnet-4-5","providerID":"anthropic","mode":"build","path":{"cwd":"/tmp/x","root":"/tmp/x"},"system":[],"cost":0.0421,"tokens":{"input":1200,"output":800,"reasoning":0,"cache":{"read":10000,"write":2000}}}"#;
    const PROMPT: &str =
        r#"{"id":"msg_00","sessionID":"ses_01","role":"user","time":{"created":1793095199000}}"#;

    #[test]
    fn reads_an_assistant_message() {
        let r = parse(REPLY).unwrap();
        assert_eq!(r.tokens, 14_000);
        assert_eq!(r.model, "claude-sonnet-4-5");
        assert!(parse(PROMPT).is_none());
        assert!(parse("{").is_none());

        let now = parse_time("2026-10-27T12:00:00Z").unwrap();
        let v = view(&Tally::new(&[r], now), now);
        assert_eq!(v.summary, "14.0k today · 14.0k this week · $0.04");
    }

    #[test]
    fn reads_database_rows() {
        // Shaped like `sqlite3 -json` over OpenCode's message table: the data
        // column holds the message without its id and session.
        let rows = r#"[{"data":"{\"role\":\"assistant\",\"time\":{\"created\":1793095200000,\"completed\":1793095212000},\"parentID\":\"msg_00\",\"modelID\":\"gpt-5\",\"providerID\":\"openai\",\"mode\":\"build\",\"cost\":0.01,\"tokens\":{\"input\":100,\"output\":50,\"reasoning\":10,\"cache\":{\"read\":0,\"write\":0}}}"},
{"data":"{\"role\":\"user\",\"time\":{\"created\":1793095199000}}"}]"#;
        let records = parse_rows(rows);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tokens, 160);
        assert_eq!(records[0].model, "gpt-5");
        assert_eq!(records[0].at, 1_793_095_200);
        assert!(parse_rows("").is_empty());
        assert!(parse_rows("\n").is_empty());
    }

    #[tokio::test]
    async fn reads_a_database() {
        let dir = std::env::temp_dir().join(format!("opencode-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("opencode.db");
        let now = now_secs();
        let reply = REPLY.replace("1793095200000", &(now * 1000).to_string());
        let sql = format!(
            "CREATE TABLE message (id text PRIMARY KEY, session_id text NOT NULL, \
             time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL); \
             INSERT INTO message VALUES ('msg_01', 'ses_01', {ms}, {ms}, '{reply}'); \
             INSERT INTO message VALUES ('msg_old', 'ses_01', 1000, 1000, '{REPLY}');",
            ms = now * 1000
        );
        // Needs the sqlite3 CLI; skip where it is missing.
        let made = std::process::Command::new("sqlite3")
            .arg(&db)
            .arg(&sql)
            .status()
            .is_ok_and(|s| s.success());
        if made {
            assert_eq!(databases(&dir), vec![db.clone()]);
            let records = read_database(&db, now).await.unwrap();
            assert_eq!(records.len(), 1, "the old row is outside the week");
            assert_eq!(records[0].tokens, 14_000);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_a_storage_folder() {
        let dir = std::env::temp_dir().join(format!("opencode-test-{}", std::process::id()));
        let session = dir.join("storage/message/ses_01");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("msg_00.json"), PROMPT).unwrap();
        std::fs::write(session.join("msg_01.json"), REPLY).unwrap();
        let records = read_messages(&dir, now_secs()).unwrap();
        std::fs::remove_dir_all(dir.join("storage")).unwrap();
        // Installed but never used: nothing to count, not an error.
        let unused = read_messages(&dir, now_secs()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(records.len(), 1);
        assert!(unused.is_empty());
        assert!(read_messages(Path::new("/nonexistent"), 0).is_err());
    }
}
