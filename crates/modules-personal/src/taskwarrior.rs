//! Pending Taskwarrior tasks, read with `task export`, most urgent first.
//!
//! ```yaml
//! modules:
//!   taskwarrior:
//!     config:
//!       filter: ["status:pending", "+work"]   # any task filter
//!       limit: 15
//! ```

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{Action, CatalogEntry, Manifest, Requirements, Size, View};
use dashboard_host::Module;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

use crate::{Task, manifest, tasks_view, today};

/// How long `task export` may take.
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The task filter, one word per item, e.g. `["status:pending", "+work"]`.
    /// Waiting tasks are left out unless the filter asks for them.
    filter: Vec<String>,
    /// Rows shown.
    limit: usize,
    /// The task program, e.g. a full path.
    task: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            filter: vec!["status:pending".into()],
            limit: 20,
            task: "task".into(),
        }
    }
}

/// One task of `task export`. Dates look like `20261030T040000Z`.
#[derive(Debug, Deserialize)]
struct Exported {
    #[serde(default)]
    id: u64,
    description: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    due: Option<String>,
    #[serde(default)]
    scheduled: Option<String>,
    #[serde(default)]
    wait: Option<String>,
    /// Set while the task is started.
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    urgency: f64,
    #[serde(default)]
    annotations: Vec<Annotation>,
}

#[derive(Debug, Deserialize)]
struct Annotation {
    description: String,
}

#[derive(Default)]
pub struct Taskwarrior {
    config: Config,
}

#[async_trait]
impl Module for Taskwarrior {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest(
            "taskwarrior",
            "Taskwarrior",
            60_000,
            5_000,
            Size::Medium,
            20,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let json = export(c).await?;
        let tz = TimeZone::system();
        view(&json, Timestamp::now(), &tz, c.limit)
    }
}

/// Runs `task <filter> export`.
async fn export(c: &Config) -> Result<String> {
    let mut cmd = Command::new(&c.task);
    // No hooks' chatter, no prompts, no colour, and never write (no gc).
    cmd.args([
        "rc.verbose=nothing",
        "rc.confirmation=off",
        "rc.color=off",
        "rc.gc=off",
        "rc.hooks=off",
    ])
    .args(&c.filter)
    .arg("export")
    .stdin(Stdio::null())
    .kill_on_drop(true);
    let out = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Err(_) => bail!("`task export` did not finish in {}s", TIMEOUT.as_secs()),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!("the task program was not found; install Taskwarrior or set `task:`")
        }
        Ok(out) => out.with_context(|| format!("running {}", c.task))?,
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "task export failed: {}",
            err.lines().last().unwrap_or("no message").trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `20261030T040000Z`, the form of Taskwarrior's dates.
fn timestamp(value: &str) -> Option<Timestamp> {
    let dt = jiff::fmt::strtime::parse("%Y%m%dT%H%M%SZ", value)
        .ok()?
        .to_datetime()
        .ok()?;
    Some(dt.to_zoned(TimeZone::UTC).ok()?.timestamp())
}

/// A Taskwarrior date as a day in `tz`.
fn date(value: &str, tz: &TimeZone) -> Option<Date> {
    Some(timestamp(value)?.to_zoned(tz.clone()).date())
}

/// The first link in a task's description or notes.
fn first_url(t: &Exported) -> Option<String> {
    std::iter::once(&t.description)
        .chain(t.annotations.iter().map(|a| &a.description))
        .flat_map(|s| s.split_whitespace())
        .find(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(String::from)
}

/// The tile from `task export` output, in urgency order.
fn view(json: &str, now: Timestamp, tz: &TimeZone, limit: usize) -> Result<View> {
    let mut exported: Vec<Exported> =
        serde_json::from_str(json.trim()).context("reading task export's JSON")?;
    // Only pending tasks; one waiting until later is hidden, as `task next`
    // does. (Recurring tasks show as their pending instances.)
    exported.retain(|t| {
        t.status == "pending"
            && t.wait
                .as_deref()
                .and_then(timestamp)
                .is_none_or(|w| w <= now)
    });
    exported.sort_by(|a, b| b.urgency.total_cmp(&a.urgency));
    let tasks: Vec<Task> = exported
        .iter()
        .map(|t| {
            let mut tags = vec![];
            if t.start.is_some() {
                tags.push("active".into());
            }
            tags.extend(t.project.iter().map(|p| format!("pro:{p}")));
            tags.extend(t.tags.iter().cloned());
            Task {
                title: if t.id > 0 {
                    format!("{} {}", t.id, t.description)
                } else {
                    t.description.clone()
                },
                due: t.due.as_deref().and_then(|d| date(d, tz)),
                scheduled: t.scheduled.as_deref().and_then(|d| date(d, tz)),
                tags,
                source: Some(format!("urgency {:.1}", t.urgency)),
                action: first_url(t).map(|url| Action::OpenUrl { url }),
                ..Task::default()
            }
        })
        .collect();
    let today = now.to_zoned(tz.clone()).date();
    Ok(tasks_view(&tasks, today, limit))
}

pub fn catalog() -> CatalogEntry {
    let now = Timestamp::now();
    let tz = TimeZone::system();
    let today = today();
    let d = |days: i64| {
        today
            .checked_add(days.days())
            .unwrap_or(today)
            .strftime("%Y%m%dT120000Z")
            .to_string()
    };
    let sample = format!(
        r#"[
{{"id":3,"description":"Fix the flaky login test","status":"pending","project":"web","tags":["bug"],"priority":"H","due":"{}","start":"{}","urgency":19.2,"annotations":[{{"entry":"{}","description":"https://github.com/acme/web/issues/812"}}]}},
{{"id":1,"description":"Review the hiring loop feedback","status":"pending","project":"team","due":"{}","urgency":12.4}},
{{"id":5,"description":"Update the on-call runbook","status":"pending","project":"ops","scheduled":"{}","urgency":6.1}},
{{"id":2,"description":"Try the new espresso grinder","status":"pending","tags":["home"],"priority":"L","urgency":1.8}}
]"#,
        d(-1),
        d(-1),
        d(-1),
        d(0),
        d(2),
    );
    let demo = view(&sample, now, &tz, 20).expect("demo export parses");
    CatalogEntry {
        description: "Pending Taskwarrior tasks, most urgent first, read with `task export`. \
                      Shows due dates, projects and tags; a link in a task or its notes \
                      opens from the tile."
            .into(),
        tags: crate::strings(&["tasks", "taskwarrior", "cli", "personal"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["task"]),
            env: vec![],
            hosts: vec![],
        },
        ..crate::entry("taskwarrior", "Taskwarrior", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{Status, Widget};

    const SAMPLE: &str = include_str!("../testdata/taskwarrior/export.json");

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn parses_an_export() {
        let tz = TimeZone::get("Europe/Berlin").unwrap();
        let v = view(SAMPLE, at("2026-10-27T09:00:00Z"), &tz, 10).unwrap();
        let Widget::List { items, .. } = &v.root else {
            panic!()
        };
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        // Urgency order; the completed task and the one waiting until
        // November are left out.
        assert_eq!(
            titles,
            [
                "3 Fix the flaky login test",
                "1 Review the hiring loop feedback",
                "5 Update the on-call runbook",
                "2 Try the new espresso grinder",
            ]
        );
        assert_eq!(v.summary, "1 overdue · 1 due today · 4 open");
        assert_eq!(v.status, Status::Warn);
        let first = &items[0];
        assert_eq!(first.tags, ["active", "pro:web", "bug"]);
        assert_eq!(first.status, Some(Status::Error));
        assert_eq!(first.subtitle.as_deref(), Some("overdue 1d · urgency 19.2"));
        assert_eq!(
            first.action,
            Some(Action::OpenUrl {
                url: "https://github.com/acme/web/issues/812".into()
            })
        );
        // Due at 23:30 UTC on the 26th is the 27th in Berlin.
        assert_eq!(items[1].status, Some(Status::Warn));
        assert_eq!(
            items[2].subtitle.as_deref(),
            Some("scheduled Thu · urgency 6.1")
        );
        assert_eq!(items[3].action, None);
    }

    #[test]
    fn bad_json_is_an_error() {
        let tz = TimeZone::UTC;
        assert!(view("not json", Timestamp::now(), &tz, 10).is_err());
        let v = view("[]", Timestamp::now(), &tz, 10).unwrap();
        assert_eq!(v.summary, "0 open");
    }

    #[test]
    fn dates_in_a_zone() {
        let ny = TimeZone::get("America/New_York").unwrap();
        assert_eq!(
            date("20261030T030000Z", &ny),
            Some(jiff::civil::date(2026, 10, 29))
        );
        assert_eq!(date("bad", &ny), None);
    }
}
