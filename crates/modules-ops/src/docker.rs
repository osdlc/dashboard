//! Running Docker containers, read with the `docker` CLI: name, image, status
//! and, from `docker stats`, CPU and memory.
//!
//! ```yaml
//! modules:
//!   docker:
//!     config:
//!       stats: true      # CPU and memory columns (`docker stats` takes about 2 s)
//!       limit: 20        # rows shown
//!       docker: docker   # the docker program, e.g. a full path
//! ```
//!
//! The same widget is also written as a TypeScript and a Python plugin
//! (`plugins/ext/docker-ts`, `plugins/ext/docker-py`) for the native-vs-plugin
//! benchmark. All three build the same view from the same docker output, so a
//! change to the view here goes into both plugins too.

use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, Cell, Column, Manifest, Requirements, Row, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

/// How long one docker command may take. A render runs up to two (ps, then
/// stats), so 2 x 10 s stays under the plugin host's 30 s limit
/// (crates/host/src/plugin.rs) and the TS and Python ports, which use the same
/// value, time out the same way.
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Show CPU and memory, from `docker stats` (about 2 s per refresh).
    stats: bool,
    /// Rows shown.
    limit: usize,
    /// The docker program, e.g. a full path.
    docker: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stats: true,
            limit: 20,
            docker: "docker".into(),
        }
    }
}

/// One line of `docker ps --format '{{json .}}'`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Container {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    status: String,
}

/// One line of `docker stats --no-stream --format '{{json .}}'`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Stats {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "CPUPerc")]
    cpu_perc: String,
    mem_usage: String,
}

#[derive(Default)]
pub struct Docker {
    config: Config,
}

#[async_trait]
impl Module for Docker {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest("docker", "Docker", 15_000, 5_000, Size::Wide, 20))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let ps = match docker(&c.docker, &["ps", "--format", "{{json .}}"]).await {
            Ok(out) => out,
            Err(failure) => return Ok(failure.view()),
        };
        let containers: Vec<Container> = parse_lines(&ps);
        let stats = if c.stats && !containers.is_empty() {
            // Without stats the tile still shows the containers.
            docker(
                &c.docker,
                &["stats", "--no-stream", "--format", "{{json .}}"],
            )
            .await
            .ok()
            .map(|out| parse_lines(&out))
        } else {
            None
        };
        Ok(view(&containers, stats.as_deref(), c.limit))
    }
}

/// Why a docker command gave no output.
#[derive(Debug, PartialEq)]
enum Failure {
    NotFound,
    TimedOut,
    /// The command failed; the last line it wrote to stderr.
    Failed(String),
}

impl Failure {
    /// The error tile. Rendered as a view, not an error, so the tile doesn't
    /// keep showing the last containers while the daemon is down.
    fn view(&self) -> View {
        let (summary, text) = match self {
            Failure::NotFound => (
                "docker not found",
                "The docker command was not found. Install Docker, or set `docker:` in the \
                 config to the docker program."
                    .to_string(),
            ),
            Failure::TimedOut => ("docker timed out", "docker did not answer.".to_string()),
            Failure::Failed(line) if daemon_down(line) => ("Docker is not running", line.clone()),
            Failure::Failed(line) => ("docker failed", line.clone()),
        };
        View {
            status: Status::Error,
            summary: summary.into(),
            root: Widget::Text {
                text,
                markdown: false,
            },
        }
    }
}

/// What the docker CLI says when it can't reach the daemon.
fn daemon_down(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("cannot connect to the docker daemon")
        || s.contains("failed to connect to the docker api")
        || s.contains("is the docker daemon running")
}

/// Runs `docker <args>` and returns its stdout.
async fn docker(program: &str, args: &[&str]) -> Result<String, Failure> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let out = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(Failure::NotFound),
        Ok(Err(e)) => return Err(Failure::Failed(format!("running docker: {e}"))),
        Err(_) => return Err(Failure::TimedOut),
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Failure::Failed(last_line(&stderr, args[0])));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The last non-empty line, or a stand-in when there is none.
fn last_line(stderr: &str, command: &str) -> String {
    match stderr.lines().map(str::trim).rfind(|l| !l.is_empty()) {
        Some(line) => line.to_string(),
        None => format!("docker {command} failed"),
    }
}

/// One JSON object per line; lines that aren't one are skipped.
fn parse_lines<T: for<'de> Deserialize<'de>>(text: &str) -> Vec<T> {
    text.lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

fn unhealthy(c: &Container) -> bool {
    c.status.contains("(unhealthy)")
}

fn restarting(c: &Container) -> bool {
    c.state == "restarting"
}

/// The tile. `stats` is `None` when CPU and memory are off or unavailable.
fn view(containers: &[Container], stats: Option<&[Stats]>, limit: usize) -> View {
    let sick = containers.iter().filter(|c| unhealthy(c)).count();
    let looping = containers.iter().filter(|c| restarting(c)).count();
    let mut summary = vec![match containers.len() {
        1 => "1 container".to_string(),
        n => format!("{n} containers"),
    }];
    if sick > 0 {
        summary.push(format!("{sick} unhealthy"));
    }
    if looping > 0 {
        summary.push(format!("{looping} restarting"));
    }
    let status = if sick + looping > 0 {
        Status::Warn
    } else {
        Status::Ok
    };
    if containers.is_empty() {
        return View {
            status,
            summary: "no containers running".into(),
            root: Widget::Text {
                text: "No containers running.".into(),
                markdown: false,
            },
        };
    }

    let mut columns = vec![
        column("name", "Name"),
        column("image", "Image"),
        column("status", "Status"),
    ];
    if stats.is_some() {
        columns.push(column("cpu", "CPU"));
        columns.push(column("mem", "Memory"));
    }
    let rows = containers
        .iter()
        .take(limit)
        .map(|c| {
            let mut cells = vec![
                ("name", c.names.clone()),
                ("image", c.image.clone()),
                ("status", c.status.clone()),
            ];
            if let Some(stats) = stats {
                // Matched on the short ID: ps and stats may print it at different lengths.
                let s = stats.iter().find(|s| short(&s.id) == short(&c.id));
                cells.push(("cpu", s.map_or("-".into(), |s| s.cpu_perc.clone())));
                // "10.2MiB / 7.66GiB" -> "10.2MiB"
                let mem = s.map(|s| s.mem_usage.split(" / ").next().unwrap_or("").to_string());
                cells.push(("mem", mem.unwrap_or_else(|| "-".into())));
            }
            Row {
                cells: cells
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), Cell::Text(v)))
                    .collect(),
                status: Some(if unhealthy(c) || restarting(c) {
                    Status::Warn
                } else {
                    Status::Ok
                }),
                action: None,
            }
        })
        .collect();
    View {
        status,
        summary: summary.join(" · "),
        root: Widget::Table { columns, rows },
    }
}

fn column(key: &str, label: &str) -> Column {
    Column {
        key: key.into(),
        label: label.into(),
    }
}

fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

pub fn catalog() -> CatalogEntry {
    let containers = parse_lines(include_str!("../testdata/docker/ps.jsonl"));
    let stats: Vec<Stats> = parse_lines(include_str!("../testdata/docker/stats.jsonl"));
    let demo = view(&containers, Some(&stats), 20);
    CatalogEntry {
        description: "Running Docker containers: name, image, status, CPU and memory. \
                      Shows an error tile when the Docker daemon is not running."
            .into(),
        tags: crate::strings(&["docker", "containers", "ops", "local"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["docker"]),
            env: vec![],
            hosts: vec![],
        },
        ..crate::entry("docker", "Docker", Size::Wide, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = include_str!("../testdata/docker/ps.jsonl");
    const STATS: &str = include_str!("../testdata/docker/stats.jsonl");

    fn text(row: &Row, key: &str) -> String {
        match &row.cells[key] {
            Cell::Text(t) => t.clone(),
            other => panic!("{key}: {other:?}"),
        }
    }

    #[test]
    fn parses_ps_and_stats() {
        let containers: Vec<Container> = parse_lines(PS);
        let stats: Vec<Stats> = parse_lines(STATS);
        assert_eq!(containers.len(), 4);
        assert_eq!(stats.len(), 3);
        let v = view(&containers, Some(&stats), 20);
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.summary, "4 containers · 1 unhealthy · 1 restarting");
        let Widget::Table { columns, rows } = &v.root else {
            panic!("table")
        };
        assert_eq!(columns.len(), 5);
        assert_eq!(text(&rows[0], "name"), "web");
        assert_eq!(text(&rows[0], "image"), "nginx:1.27");
        assert_eq!(text(&rows[0], "cpu"), "0.02%");
        assert_eq!(text(&rows[0], "mem"), "10.2MiB");
        assert_eq!(rows[0].status, Some(Status::Ok));
        assert_eq!(rows[2].status, Some(Status::Warn));
        // Restarting: no stats line.
        assert_eq!(text(&rows[3], "cpu"), "-");
        assert_eq!(rows[3].status, Some(Status::Warn));
    }

    #[test]
    fn without_stats_has_no_cpu_column() {
        let containers: Vec<Container> = parse_lines(PS);
        let v = view(&containers, None, 2);
        let Widget::Table { columns, rows } = &v.root else {
            panic!("table")
        };
        assert_eq!(columns.len(), 3);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].cells.contains_key("cpu"));
        // The summary counts every container, not only the rows shown.
        assert!(v.summary.starts_with("4 containers"), "{}", v.summary);
    }

    #[test]
    fn skips_lines_that_are_not_json() {
        let containers: Vec<Container> = parse_lines("WARNING: something\n\n{\"bad\": 1}\n");
        assert!(containers.is_empty());
        let v = view(&containers, None, 20);
        assert_eq!(v.status, Status::Ok);
        assert_eq!(v.summary, "no containers running");
    }

    #[test]
    fn daemon_down_is_an_error_tile() {
        let v = Failure::Failed(last_line(
            "\nCannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?\n",
            "ps",
        ))
        .view();
        assert_eq!(v.status, Status::Error);
        assert_eq!(v.summary, "Docker is not running");
        assert_eq!(
            Failure::Failed(last_line("", "ps")).view().summary,
            "docker failed"
        );
        assert_eq!(last_line("  ", "ps"), "docker ps failed");
    }

    #[tokio::test]
    async fn missing_program_is_an_error_tile() {
        let mut m = Docker::default();
        m.init(serde_json::json!({ "docker": "/no/such/docker" }))
            .await
            .unwrap();
        let v = m.render().await.unwrap();
        assert_eq!(v.status, Status::Error);
        assert_eq!(v.summary, "docker not found");
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
