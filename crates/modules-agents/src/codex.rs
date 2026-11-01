//! OpenAI Codex limits and token use.
//!
//! Live numbers come from `codex app-server` (JSON-RPC over stdio, Codex's
//! own login; experimental). If that fails, the module falls back to Codex's
//! session logs (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`), which need no
//! network but are only as fresh as the last Codex request.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Bar, CatalogEntry, Manifest, Requirements, Size, Status, ValueFormat, View, Widget,
};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::{Window, count, day_label, now_secs, worst};
use dashboard_modules_common::expand_home;
use dashboard_modules_common::manifest;
use dashboard_modules_common::time::{ago, demo_time, parse_time};

/// Session files looked at, newest first, before giving up.
const MAX_FILES: usize = 30;
/// Days shown in the tokens-per-day bars.
const DAYS: usize = 7;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Codex's home; default `$CODEX_HOME` or `~/.codex`.
    home: Option<String>,
    /// `live` (default: ask `codex app-server`, else read the logs) or `logs`.
    source: Option<String>,
}

#[derive(Default)]
pub struct Codex {
    home: PathBuf,
    live: bool,
}

/// What the tile shows, from either source.
#[derive(Debug, Default)]
struct Snapshot {
    plan: Option<String>,
    limits: Vec<Limit>,
    credits: Option<String>,
    /// When the numbers are from the logs: the time of that request.
    as_of: Option<String>,
    last_session_tokens: Option<u64>,
    /// (`YYYY-MM-DD`, tokens), oldest first.
    daily: Vec<(String, u64)>,
    lifetime_tokens: Option<u64>,
    streak_days: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limit {
    #[serde(alias = "used_percent")]
    used_percent: f64,
    #[serde(alias = "window_minutes", alias = "windowDurationMins")]
    window_minutes: Option<i64>,
    #[serde(alias = "resets_at")]
    resets_at: Option<i64>,
}

#[async_trait]
impl Module for Codex {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        let config: Config = if config.is_null() {
            Config::default()
        } else {
            serde_json::from_value(config)?
        };
        self.home = match config.home.or_else(|| std::env::var("CODEX_HOME").ok()) {
            Some(h) => expand_home(&h),
            None => expand_home("~/.codex"),
        };
        self.live = match config.source.as_deref() {
            None | Some("live") => true,
            Some("logs") => false,
            Some(other) => bail!("codex source {other:?}: use `live` or `logs`"),
        };
        Ok(manifest("codex", "Codex", 60_000, 15_000, Size::Medium, 45))
    }

    async fn render(&mut self) -> Result<View> {
        let snapshot = if self.live {
            match live().await {
                Ok(s) => s,
                Err(live_err) => from_logs(&self.home)
                    .with_context(|| format!("codex app-server failed ({live_err:#}); logs"))?,
            }
        } else {
            from_logs(&self.home)?
        };
        Ok(view(&snapshot, now_secs()))
    }
}

// ---- live: codex app-server ----

async fn live() -> Result<Snapshot> {
    // On Windows npm installs `codex.cmd`, which needs the PATHEXT lookup.
    let argv = dashboard_host::process::resolve_argv(&["codex".into(), "app-server".into()], None);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("running codex (is the Codex CLI installed?)")?;
    // Killing the `.cmd` shim alone would leave its `node` running.
    #[cfg(windows)]
    let _tree = dashboard_host::process::KillTree::new(&child);
    let mut rpc = Rpc {
        stdin: child.stdin.take().context("codex stdin")?,
        stdout: BufReader::new(child.stdout.take().context("codex stdout")?).lines(),
        next_id: 0,
    };
    let work = async {
        rpc.call::<Value>(
            "initialize",
            json!({ "clientInfo": { "name": "dashboard", "title": null, "version": env!("CARGO_PKG_VERSION") }, "capabilities": null }),
        )
        .await?;
        rpc.notify("initialized").await?;
        let limits: LiveLimits = rpc
            .call(
                "account/rateLimits/read",
                json!({ "excludeResetCreditDetails": true }),
            )
            .await?;
        // Token history is a nice extra; the limits alone are enough.
        let usage: Option<LiveUsage> = rpc.call("account/usage/read", json!({})).await.ok();
        Ok::<_, anyhow::Error>((limits, usage))
    };
    let (limits, usage) = tokio::time::timeout(Duration::from_secs(20), work)
        .await
        .context("codex app-server did not answer in 20s")??;
    let _ = child.kill().await;

    let snap = limits.rate_limits;
    let mut out = Snapshot {
        plan: snap.plan_type,
        limits: [snap.primary, snap.secondary]
            .into_iter()
            .flatten()
            .collect(),
        credits: snap.credits.and_then(credits_text),
        ..Snapshot::default()
    };
    if let Some(u) = usage {
        out.daily = u
            .daily_usage_buckets
            .unwrap_or_default()
            .into_iter()
            .map(|b| (b.start_date, b.tokens))
            .collect();
        out.lifetime_tokens = u.summary.lifetime_tokens;
        out.streak_days = u.summary.current_streak_days;
    }
    Ok(out)
}

/// Newline-delimited JSON-RPC, as `codex app-server` speaks it (no
/// `jsonrpc` field). Notifications that arrive between replies are skipped.
struct Rpc {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    next_id: u64,
}

impl Rpc {
    async fn call<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> Result<T> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({ "id": id, "method": method, "params": params }))
            .await?;
        while let Some(line) = self.stdout.next_line().await? {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(err) = msg.get("error") {
                bail!("{method}: {}", err.get("message").unwrap_or(err));
            }
            return serde_json::from_value(msg.get("result").cloned().unwrap_or(Value::Null))
                .with_context(|| format!("reading the {method} reply"));
        }
        bail!("codex app-server exited")
    }

    async fn notify(&mut self, method: &str) -> Result<()> {
        self.write(json!({ "method": method })).await
    }

    async fn write(&mut self, msg: Value) -> Result<()> {
        let mut line = msg.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        Ok(self.stdin.flush().await?)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveLimits {
    rate_limits: LiveSnapshot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveSnapshot {
    primary: Option<Limit>,
    secondary: Option<Limit>,
    credits: Option<Credits>,
    plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credits {
    #[serde(alias = "has_credits")]
    has_credits: bool,
    #[serde(default)]
    unlimited: bool,
    balance: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveUsage {
    summary: UsageSummary,
    daily_usage_buckets: Option<Vec<DailyBucket>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageSummary {
    lifetime_tokens: Option<u64>,
    current_streak_days: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DailyBucket {
    start_date: String,
    tokens: u64,
}

fn credits_text(c: Credits) -> Option<String> {
    if c.unlimited {
        Some("unlimited credits".into())
    } else if c.has_credits {
        Some(format!("{} credits", c.balance.unwrap_or_default()))
    } else {
        None
    }
}

// ---- fallback: session logs ----

#[derive(Debug, Deserialize)]
struct Event {
    timestamp: String,
    payload: Payload,
}

#[derive(Debug, Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    info: Option<Info>,
    #[serde(default)]
    rate_limits: Option<LogLimits>,
}

#[derive(Debug, Deserialize)]
struct Info {
    total_token_usage: Tokens,
}

#[derive(Debug, Deserialize)]
struct Tokens {
    total_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct LogLimits {
    primary: Option<Limit>,
    secondary: Option<Limit>,
    credits: Option<Credits>,
    plan_type: Option<String>,
}

fn from_logs(home: &Path) -> Result<Snapshot> {
    let sessions = home.join("sessions");
    if !sessions.is_dir() {
        bail!(
            "no Codex sessions in {} (is Codex installed and used?)",
            sessions.display()
        );
    }
    let (event, limits) = newest_limits(&sessions)?;
    Ok(Snapshot {
        plan: limits.plan_type,
        limits: [limits.primary, limits.secondary]
            .into_iter()
            .flatten()
            .collect(),
        credits: limits.credits.and_then(credits_text),
        last_session_tokens: event.payload.info.map(|i| i.total_token_usage.total_tokens),
        as_of: Some(event.timestamp),
        ..Snapshot::default()
    })
}

/// The last `token_count` event that carries rate limits, from the newest
/// session file that has one.
fn newest_limits(sessions: &Path) -> Result<(Event, LogLimits)> {
    for file in newest_files(sessions)?.into_iter().take(MAX_FILES) {
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        for line in text.lines().rev() {
            if !line.contains("\"token_count\"") {
                continue;
            }
            let Ok(mut event) = serde_json::from_str::<Event>(line) else {
                continue;
            };
            if event.payload.kind == "token_count"
                && let Some(limits) = event.payload.rate_limits.take()
            {
                return Ok((event, limits));
            }
        }
    }
    bail!("no rate limits in the latest Codex sessions yet; run Codex once")
}

/// Session files, newest first. Directory and file names sort by date.
fn newest_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries.into_iter().rev() {
        if path.is_dir() {
            out.extend(newest_files(&path)?);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
        if out.len() >= MAX_FILES {
            break;
        }
    }
    Ok(out)
}

// ---- view ----

fn view(s: &Snapshot, now: i64) -> View {
    let windows: Vec<Window> = s
        .limits
        .iter()
        .map(|l| {
            // A window that reset after the numbers were taken starts again at 0.
            let reset = l.resets_at.is_some_and(|r| r <= now);
            Window {
                label: l
                    .window_minutes
                    .map(window_name)
                    .unwrap_or_else(|| "Limit".into()),
                used_pct: if reset { 0.0 } else { l.used_percent },
                resets_at: l.resets_at.filter(|_| !reset),
                length: l.window_minutes.map(|m| m * 60),
                note: reset.then(|| "reset since".into()),
            }
        })
        .collect();

    let mut children: Vec<Widget> = windows.iter().map(|w| w.gauge(now)).collect();
    if !s.daily.is_empty() {
        let shown = &s.daily[s.daily.len().saturating_sub(DAYS)..];
        children.push(Widget::Bars {
            label: Some("Tokens per day".into()),
            unit: None,
            format: ValueFormat::Count,
            max: None,
            bars: shown
                .iter()
                .map(|(date, tokens)| Bar {
                    label: day_label(date),
                    value: *tokens as f64,
                    status: None,
                })
                .collect(),
        });
    }
    let mut facts = vec![];
    if let Some(c) = &s.credits {
        facts.push(c.clone());
    }
    if let Some(t) = s.lifetime_tokens {
        facts.push(format!("{} tokens all time", count(t)));
    }
    if let Some(d) = s.streak_days.filter(|d| *d > 1) {
        facts.push(format!("{d}-day streak"));
    }
    if let Some(t) = s.last_session_tokens {
        facts.push(format!("last session {} tokens", count(t)));
    }
    if let Some(ts) = &s.as_of {
        facts.push(format!("from the logs, as of {}", ago(ts)));
    }
    if !facts.is_empty() {
        children.push(Widget::Text {
            text: facts.join(" · "),
            markdown: false,
        });
    }

    let plan = s.plan.as_deref().unwrap_or("unknown plan");
    let used = windows
        .iter()
        .map(|w| format!("{:.0}% {}", w.used_pct, w.label.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" · ");
    // Old log numbers are a hint, not a warning: Codex just hasn't run lately.
    let stale = s
        .as_of
        .as_deref()
        .is_some_and(|ts| parse_time(ts).is_none_or(|t| now - t > 7 * 86_400));
    View {
        status: if stale {
            Status::Unknown
        } else {
            worst(&windows)
        },
        summary: [plan, &used].join(" · "),
        root: Widget::Column { children },
    }
}

pub fn catalog() -> CatalogEntry {
    let now = now_secs();
    let tokens = [412_000, 1_180_000, 96_000, 0, 2_350_000, 870_000, 1_420_000];
    let snapshot = Snapshot {
        plan: Some("plus".into()),
        limits: vec![
            Limit {
                used_percent: 34.0,
                window_minutes: Some(300),
                resets_at: Some(now + 2 * 3600 + 40 * 60),
            },
            Limit {
                used_percent: 61.0,
                window_minutes: Some(10_080),
                resets_at: Some(now + 3 * 86_400 + 5 * 3600),
            },
        ],
        daily: tokens
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let date = demo_time((6 - i as i64) * 86_400);
                (date[..10].to_string(), *t)
            })
            .collect(),
        lifetime_tokens: Some(48_300_000),
        streak_days: Some(5),
        ..Snapshot::default()
    };
    CatalogEntry {
        description: "OpenAI Codex limits (5-hour and weekly) and tokens per day, from \
                      `codex app-server` or Codex's session logs."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "openai"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["codex"]),
            env: vec![],
            hosts: crate::strings(&["chatgpt.com"]),
        },
        ..crate::entry("codex", "Codex", Size::Medium, view(&snapshot, now))
    }
}

fn window_name(minutes: i64) -> String {
    match minutes {
        300 => "5 hours".into(),
        10_080 => "Weekly".into(),
        m if m % 1440 == 0 => format!("{} days", m / 1440),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"timestamp":"2026-10-25T02:32:38.940Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":78890,"cached_input_tokens":45568,"output_tokens":218,"reasoning_output_tokens":0,"total_tokens":79108},"model_context_window":475000},"rate_limits":{"limit_id":"codex","primary":{"used_percent":12.0,"window_minutes":300,"resets_at":1793400000},"secondary":{"used_percent":40.0,"window_minutes":10080,"resets_at":1793481262},"plan_type":"plus"}}}"#;

    #[test]
    fn reads_limits_from_a_session_file() {
        let dir = std::env::temp_dir().join(format!("codex-test-{}", std::process::id()));
        let day = dir.join("sessions/2026/10/25");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(
            day.join("rollout-a.jsonl"),
            format!("{{}}\n{LINE}\n{{\"x\":1}}\n"),
        )
        .unwrap();
        let snap = from_logs(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let now = 1_793_390_000; // before both resets
        let v = view(&snap, now);
        assert_eq!(v.summary, "plus · 12% 5 hours · 40% weekly");
        let Widget::Column { children } = v.root else {
            panic!()
        };
        assert_eq!(children.len(), 3);

        // After the 5-hour reset that window reads 0.
        let v = view(&snap, 1_793_450_000);
        assert!(v.summary.contains("0% 5 hours"), "{}", v.summary);
    }

    #[test]
    fn reads_app_server_replies() {
        let limits: LiveLimits = serde_json::from_value(json!({
            "ordinaryUsageAllowed": true,
            "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 7, "windowDurationMins": 10080, "resetsAt": 1793684457 },
                "secondary": null,
                "credits": { "hasCredits": false, "unlimited": false, "balance": "0" },
                "planType": "plus"
            }
        }))
        .unwrap();
        assert_eq!(
            limits.rate_limits.primary.unwrap().window_minutes,
            Some(10_080)
        );
        let usage: LiveUsage = serde_json::from_value(json!({
            "summary": { "lifetimeTokens": 452166538, "currentStreakDays": 2 },
            "dailyUsageBuckets": [{ "startDate": "2026-10-24", "tokens": 278305 }]
        }))
        .unwrap();
        assert_eq!(usage.daily_usage_buckets.unwrap()[0].tokens, 278_305);
        assert_eq!(day_label("2026-10-24"), "Sat 24");
        assert_eq!(count(452_166_538), "452M");
        assert_eq!(count(4_175_614), "4.2M");
    }
}
