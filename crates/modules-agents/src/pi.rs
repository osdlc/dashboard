//! Pi (the pi coding agent) token use and cost, from its session logs
//! (`~/.pi/agent/sessions/<project>/<time>_<id>.jsonl`). Each assistant
//! message there carries the tokens and cost of that reply. No network.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::now_secs;
use super::tally::{Record, Tally, recent_files};
use dashboard_modules_common::time::parse_time;
use dashboard_modules_common::{expand_home, manifest};

const DOCS: &str = "https://github.com/badlogic/pi-mono";

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Pi's agent folder; default `$PI_CODING_AGENT_DIR` or `~/.pi/agent`.
    #[schemars(extend("x-path" = true))]
    home: Option<String>,
}

#[derive(Default)]
pub struct Pi {
    home: PathBuf,
}

#[async_trait]
impl Module for Pi {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        let config: Config = if config.is_null() {
            Config::default()
        } else {
            serde_json::from_value(config)?
        };
        self.home = expand_home(
            &config
                .home
                .or_else(|| std::env::var("PI_CODING_AGENT_DIR").ok())
                .unwrap_or_else(|| "~/.pi/agent".into()),
        );
        Ok(manifest("pi", "Pi", 120_000, 15_000, Size::Medium, 46))
    }

    async fn render(&mut self) -> Result<View> {
        let now = now_secs();
        let records = read_sessions(&self.home, now)?;
        Ok(view(&Tally::new(&records, now), now))
    }
}

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    role: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
    /// Milliseconds since the epoch.
    #[serde(default)]
    timestamp: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    #[serde(default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    cost: Option<Cost>,
}

#[derive(Debug, Deserialize)]
struct Cost {
    #[serde(default)]
    total: f64,
}

fn read_sessions(home: &Path, now: i64) -> Result<Vec<Record>> {
    if !home.is_dir() {
        bail!("no Pi data in {} (is Pi installed?)", home.display());
    }
    // Installed but never used: no sessions folder yet, so nothing to count.
    let sessions = home.join("sessions");
    let mut out = vec![];
    for file in recent_files(&sessions, &["jsonl"], now) {
        if let Ok(text) = std::fs::read_to_string(&file) {
            out.extend(parse(&text));
        }
    }
    Ok(out)
}

/// The assistant replies of one session file. Lines that are not replies,
/// or that don't parse, are skipped.
fn parse(text: &str) -> Vec<Record> {
    text.lines()
        // Cheap filter first: most lines are tool output.
        .filter(|l| l.contains("\"usage\""))
        .filter_map(|l| serde_json::from_str::<Event>(l).ok())
        .filter(|e| e.kind == "message")
        .filter_map(|e| {
            let m = e.message?;
            let usage = m.usage.filter(|_| m.role == "assistant")?;
            let at = m
                .timestamp
                .map(|ms| ms / 1000)
                .or_else(|| e.timestamp.as_deref().and_then(parse_time))?;
            Some(Record {
                at,
                tokens: usage.total_tokens.unwrap_or(usage.input + usage.output),
                cost: usage.cost.map_or(0.0, |c| c.total),
                model: m.model.unwrap_or_else(|| "unknown".into()),
            })
        })
        .collect()
}

fn view(t: &Tally, now: i64) -> View {
    View {
        status: t.status(),
        summary: t.summary(),
        root: Widget::Column {
            children: [
                t.widgets(now),
                vec![super::link(
                    "Pi coding agent",
                    "github.com/badlogic/pi-mono",
                    DOCS,
                )],
            ]
            .concat(),
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let now = now_secs();
    let records = super::tally::demo_records(
        now,
        &[
            ("claude-sonnet-4-5", 0.6),
            ("gpt-5-codex", 0.3),
            ("qwen3:32b", 0.1),
        ],
        true,
    );
    CatalogEntry {
        description: "Pi coding agent tokens and cost per day, and the models used, from \
                      Pi's local session logs. No network."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "local"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry(
            "pi",
            "Pi",
            Size::Medium,
            view(&Tally::new(&records, now), now),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like Pi's session files (version 3), with the text cut out.
    const SAMPLE: &str = r#"{"type":"session","version":3,"id":"01a0","timestamp":"2026-10-27T06:17:47.063Z","cwd":"/tmp/x"}
{"type":"model_change","id":"5ec6","parentId":null,"timestamp":"2026-10-27T06:17:47.111Z","provider":"anthropic","modelId":"claude-sonnet-4-5"}
{"type":"message","id":"01c0","parentId":"5ec6","timestamp":"2026-10-27T06:18:34.124Z","message":{"role":"user","content":"hi","timestamp":1793082514121}}
{"type":"message","id":"9cd6","parentId":"01c0","timestamp":"2026-10-27T06:19:49.696Z","message":{"role":"assistant","content":[],"api":"anthropic-messages","provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":3358,"output":250,"cacheRead":217,"cacheWrite":0,"reasoning":0,"totalTokens":3825,"cost":{"input":0.01,"output":0.004,"cacheRead":0,"cacheWrite":0,"total":0.014}},"stopReason":"toolUse","timestamp":1793082589696}}
{"type":"message","id":"fc44","parentId":"9cd6","timestamp":"2026-10-27T06:19:50.310Z","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[],"isError":false,"timestamp":1793082590310}}
{"type":"message","id":"aa01","parentId":"fc44","timestamp":"2026-10-27T06:20:10.000Z","message":{"role":"assistant","content":[],"model":"claude-sonnet-4-5","usage":{"input":4000,"output":175,"totalTokens":4175,"cost":{"total":0.016}},"timestamp":1793082610000}}
not json
"#;

    #[test]
    fn reads_replies_from_a_session_file() {
        let records = parse(SAMPLE);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tokens, 3_825);
        assert_eq!(records[0].model, "claude-sonnet-4-5");

        let now = parse_time("2026-10-27T12:00:00Z").unwrap();
        let v = view(&Tally::new(&records, now), now);
        assert_eq!(v.summary, "8.0k today · 8.0k this week · $0.03");
        assert_eq!(v.status, dashboard_contract::Status::Ok);
    }

    #[test]
    fn reads_a_sessions_folder() {
        let dir = std::env::temp_dir().join(format!("pi-test-{}", std::process::id()));
        let project = dir.join("sessions/--tmp-x--");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("2026-10-27T06-17-47_01a0.jsonl"), SAMPLE).unwrap();
        // The file was just written, so it counts as recent whatever its contents.
        let records = read_sessions(&dir, now_secs()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(records.len(), 2);
        assert!(read_sessions(Path::new("/nonexistent"), 0).is_err());
        // Installed but never used: nothing to count, not an error.
        let empty = std::env::temp_dir().join(format!("pi-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let records = read_sessions(&empty, 0);
        std::fs::remove_dir_all(&empty).unwrap();
        assert!(records.unwrap().is_empty());
    }
}
