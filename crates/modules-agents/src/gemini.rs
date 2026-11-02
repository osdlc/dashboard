//! Gemini CLI requests against its daily quota, and tokens per day, from the
//! chats Gemini CLI saves (`~/.gemini/tmp/<project>/chats/session-*.json`).
//! No network.
//!
//! Gemini CLI documents its free quotas as requests per day, reset at midnight
//! Pacific time (1,000 a day when signed in with a Google account, 250 with a
//! free API key). The module counts saved replies since the last midnight
//! Pacific (US daylight saving rules), so the gauge is an estimate.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::tally::{Record, Tally, recent_files};
use super::{Window, now_secs};
use dashboard_modules_common::time::{parse_time, rfc3339};
use dashboard_modules_common::{expand_home, manifest};

const QUOTAS: &str =
    "https://github.com/google-gemini/gemini-cli/blob/main/docs/quota-and-pricing.md";

/// The last daily reset at or before `now`: midnight Pacific time, which is
/// 08:00 UTC in winter (PST) and 07:00 UTC in summer (PDT).
fn day_start(now: i64) -> i64 {
    let offset = if pacific_dst(now) { 7 } else { 8 } * 3600;
    (now - offset).div_euclid(86_400) * 86_400 + offset
}

/// Whether US Pacific daylight time is in effect: from 02:00 PST on the
/// second Sunday of March to 02:00 PDT on the first Sunday of November.
fn pacific_dst(t: i64) -> bool {
    let year = &rfc3339(t)[..4];
    // The day number of the first Sunday on or after the 1st of `month`.
    let first_sunday = |month: &str| {
        let day = parse_time(&format!("{year}-{month}-01T00:00:00Z")).unwrap_or(0) / 86_400;
        // Day 0 (1970-01-01) was a Thursday.
        day + (7 - (day + 4) % 7) % 7
    };
    let start = (first_sunday("03") + 7) * 86_400 + 10 * 3600;
    let end = first_sunday("11") * 86_400 + 9 * 3600;
    (start..end).contains(&t)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Gemini CLI's folder; default `$GEMINI_CLI_HOME/.gemini` or `~/.gemini`.
    #[schemars(extend("x-path" = true))]
    home: Option<String>,
    /// Requests per day of your plan; default 1000 (Google account sign-in).
    daily_limit: Option<u32>,
}

#[derive(Default)]
pub struct Gemini {
    home: PathBuf,
    limit: u32,
}

#[async_trait]
impl Module for Gemini {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        let config: Config = if config.is_null() {
            Config::default()
        } else {
            serde_json::from_value(config)?
        };
        self.home = match config.home {
            Some(h) => expand_home(&h),
            None => match std::env::var("GEMINI_CLI_HOME") {
                Ok(d) if !d.is_empty() => PathBuf::from(d).join(".gemini"),
                _ => expand_home("~/.gemini"),
            },
        };
        self.limit = config.daily_limit.unwrap_or(1000);
        if self.limit == 0 {
            bail!("gemini daily_limit must be above 0");
        }
        Ok(manifest(
            "gemini",
            "Gemini CLI",
            120_000,
            15_000,
            Size::Medium,
            46,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let now = now_secs();
        let records = read_chats(&self.home, now)?;
        Ok(view(&records, self.limit, now))
    }
}

#[derive(Debug, Deserialize)]
struct Chat {
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tokens: Option<Tokens>,
}

#[derive(Debug, Deserialize)]
struct Tokens {
    #[serde(default)]
    total: u64,
}

fn read_chats(home: &Path, now: i64) -> Result<Vec<Record>> {
    // Other Google tools (Antigravity) share ~/.gemini and its settings.json,
    // so look for Gemini CLI's own chats folder or its binary.
    let dir = home.join("tmp");
    if !dir.is_dir() && !on_path("gemini") {
        bail!(
            "no Gemini CLI data in {} (is Gemini CLI installed?)",
            home.display()
        );
    }
    Ok(recent_files(&dir, &["json"], now)
        .into_iter()
        // Each project folder also holds logs.json and checkpoints.
        .filter(|f| f.parent().is_some_and(|p| p.ends_with("chats")))
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|text| parse(&text))
        .collect())
}

/// Whether an executable named `name` is in a `PATH` folder.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// The model replies of one saved chat.
fn parse(text: &str) -> Vec<Record> {
    let Ok(chat) = serde_json::from_str::<Chat>(text) else {
        return vec![];
    };
    chat.messages
        .into_iter()
        .filter(|m| m.kind == "gemini")
        .filter_map(|m| {
            Some(Record {
                at: parse_time(m.timestamp.as_deref()?)?,
                tokens: m.tokens.map_or(0, |t| t.total),
                cost: 0.0,
                model: m.model.unwrap_or_else(|| "gemini".into()),
            })
        })
        .collect()
}

fn view(records: &[Record], limit: u32, now: i64) -> View {
    let start = day_start(now);
    let today = records.iter().filter(|r| r.at >= start).count();
    let window = Window {
        label: "Requests today".into(),
        used_pct: (100.0 * today as f64 / limit as f64).min(100.0),
        resets_at: Some(start + 86_400),
        length: Some(86_400),
        note: Some(format!("{today} of {limit}")),
    };
    let tally = Tally::new(records, now);
    let mut children = vec![window.gauge(now)];
    children.extend(tally.widgets(now));
    children.push(super::link("Quotas and pricing", "Gemini CLI docs", QUOTAS));
    View {
        status: window.status(),
        summary: format!("{today} of {limit} requests today · {}", tally.summary()),
        root: Widget::Column { children },
    }
}

pub fn catalog() -> CatalogEntry {
    let now = now_secs();
    let mut records = super::tally::demo_records(
        now,
        &[("gemini-2.5-pro", 0.8), ("gemini-2.5-flash", 0.2)],
        false,
    );
    // Some requests earlier today, for the gauge.
    for i in 0..180 {
        records.push(Record {
            at: now - 60 - i * 30,
            tokens: 0,
            cost: 0.0,
            model: "gemini-2.5-pro".into(),
        });
    }
    CatalogEntry {
        description: "Gemini CLI requests against its daily quota (an estimate) and tokens \
                      per day, from the chats Gemini CLI saves. No network."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "google", "local"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry(
            "gemini",
            "Gemini CLI",
            Size::Medium,
            view(&records, 1000, now),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like a Gemini CLI saved chat, with the text cut out.
    const CHAT: &str = r#"{
      "sessionId": "5f1c",
      "projectHash": "ab12",
      "startTime": "2026-10-27T09:00:00.000Z",
      "lastUpdated": "2026-10-27T09:05:00.000Z",
      "messages": [
        { "id": "1", "timestamp": "2026-10-27T09:00:00.000Z", "type": "user", "content": "hi" },
        { "id": "2", "timestamp": "2026-10-27T09:00:05.000Z", "type": "gemini", "content": "",
          "thoughts": [], "model": "gemini-2.5-pro",
          "tokens": { "input": 9000, "output": 300, "cached": 0, "thoughts": 200, "tool": 0, "total": 9500 } },
        { "id": "3", "timestamp": "2026-10-27T06:59:00.000Z", "type": "gemini", "content": "",
          "model": "gemini-2.5-flash",
          "tokens": { "input": 400, "output": 100, "cached": 0, "thoughts": 0, "tool": 0, "total": 500 } },
        { "id": "4", "timestamp": "2026-10-27T09:01:00.000Z", "type": "info", "content": "x" },
        { "id": "5", "type": "gemini", "content": "no timestamp: skipped alone" }
      ]
    }"#;

    #[test]
    fn counts_requests_since_the_daily_reset() {
        let records = parse(CHAT);
        assert_eq!(records.len(), 2);
        let now = parse_time("2026-10-27T12:00:00Z").unwrap();
        let v = view(&records, 4, now);
        // The 06:59 reply was before the 07:00 UTC (midnight PDT) reset.
        assert!(
            v.summary.starts_with("1 of 4 requests today · 10.0k today"),
            "{}",
            v.summary
        );
        assert_eq!(v.status, dashboard_contract::Status::Ok);
        let v = view(&vec![records[0].clone(); 4], 4, now);
        assert_eq!(v.status, dashboard_contract::Status::Error);
        assert!(parse("[]").is_empty());
    }

    #[test]
    fn resets_at_pacific_midnight() {
        let at = |ts: &str| parse_time(ts).unwrap();
        // Summer (PDT): 07:00 UTC.
        assert_eq!(
            day_start(at("2026-10-27T12:00:00Z")),
            at("2026-10-27T07:00:00Z")
        );
        assert_eq!(
            day_start(at("2026-10-27T06:00:00Z")),
            at("2026-10-26T07:00:00Z")
        );
        // Winter (PST): 08:00 UTC.
        assert_eq!(
            day_start(at("2026-12-01T12:00:00Z")),
            at("2026-12-01T08:00:00Z")
        );
        // 2026 switches: Sunday March 8 and Sunday November 1.
        assert!(!pacific_dst(at("2026-03-08T09:59:00Z")));
        assert!(pacific_dst(at("2026-03-08T10:00:00Z")));
        assert!(pacific_dst(at("2026-11-01T08:59:00Z")));
        assert!(!pacific_dst(at("2026-11-01T09:00:00Z")));
    }
}
