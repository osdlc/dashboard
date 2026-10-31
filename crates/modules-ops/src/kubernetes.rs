//! Kubernetes pods by namespace, with their status, read with `kubectl`.
//!
//! ```yaml
//! modules:
//!   kubernetes:
//!     config:
//!       namespace: shop     # one namespace; default every namespace
//!       context: prod       # a kubeconfig context; default the current one
//!       problems_only: false  # list only pods that are not running and ready
//!       limit: 30           # rows shown
//!       kubectl: kubectl    # the kubectl program, e.g. a full path
//! ```
//!
//! The status column follows `kubectl get pods`: a waiting or failed
//! container's reason (CrashLoopBackOff, ImagePullBackOff, Error...) wins
//! over the pod's phase.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Bar, CatalogEntry, Cell, Column, Manifest, Requirements, Row, Size, Status, ValueFormat, View,
    Widget,
};
use dashboard_host::{Module, now_ms};
use dashboard_modules_common::{manifest, time::parse_time};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// One namespace. Default: every namespace.
    namespace: Option<String>,
    /// A kubeconfig context. Default: the current one.
    context: Option<String>,
    /// List only pods that are not running and ready (the counts still cover all).
    problems_only: bool,
    /// Rows shown.
    limit: usize,
    /// The kubectl program, e.g. a full path.
    kubectl: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            namespace: None,
            context: None,
            problems_only: false,
            limit: 30,
            kubectl: "kubectl".into(),
        }
    }
}

/// `kubectl get pods -o json`: the parts the tile uses.
#[derive(Debug, Deserialize)]
struct PodList {
    #[serde(default)]
    items: Vec<Pod>,
}

#[derive(Debug, Deserialize)]
struct Pod {
    metadata: Metadata,
    #[serde(default)]
    status: PodStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    name: String,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    creation_timestamp: Option<String>,
    #[serde(default)]
    deletion_timestamp: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PodStatus {
    phase: String,
    reason: Option<String>,
    init_container_statuses: Vec<ContainerStatus>,
    container_statuses: Vec<ContainerStatus>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ContainerStatus {
    ready: bool,
    restart_count: u32,
    state: ContainerState,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ContainerState {
    waiting: Option<Reason>,
    terminated: Option<Reason>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Reason {
    reason: Option<String>,
}

#[derive(Default)]
pub struct Kubernetes {
    config: Config,
}

#[async_trait]
impl Module for Kubernetes {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let name = match &self.config.namespace {
            Some(ns) => format!("Pods · {ns}"),
            None => "Pods".into(),
        };
        if self.config.limit == 0 {
            bail!("`limit` must be at least 1");
        }
        Ok(manifest("kubernetes", &name, 30_000, 5_000, Size::Wide, 20))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let mut args = vec!["get", "pods", "-o", "json"];
        match &c.namespace {
            Some(ns) => args.extend(["--namespace", ns]),
            None => args.push("--all-namespaces"),
        }
        if let Some(ctx) = &c.context {
            args.extend(["--context", ctx]);
        }
        let out = match kubectl(&c.kubectl, &args).await {
            Ok(out) => out,
            Err(failure) => return Ok(failure.view()),
        };
        let pods: PodList = serde_json::from_str(&out)?;
        Ok(view(&pods.items, now_ms() / 1000, c.problems_only, c.limit))
    }
}

/// Why kubectl gave no output.
#[derive(Debug, PartialEq)]
enum Failure {
    NotFound,
    TimedOut,
    /// The command failed; the last line it wrote to stderr.
    Failed(String),
}

impl Failure {
    /// The error tile, so a cluster that went away doesn't keep showing old pods.
    fn view(&self) -> View {
        let (summary, text) = match self {
            Failure::NotFound => (
                "kubectl not found",
                "The kubectl command was not found. Install it, or set `kubectl:` in the \
                 config to the kubectl program."
                    .to_string(),
            ),
            Failure::TimedOut => ("kubectl timed out", "The cluster did not answer.".into()),
            Failure::Failed(line) if unreachable(line) => ("cluster unreachable", line.clone()),
            Failure::Failed(line) => ("kubectl failed", line.clone()),
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

/// What kubectl says when it can't reach the API server.
fn unreachable(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("unable to connect to the server")
        || s.contains("connection to the server")
        || s.contains("no configuration has been provided")
}

async fn kubectl(program: &str, args: &[&str]) -> Result<String, Failure> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let out = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(Failure::NotFound),
        Ok(Err(e)) => return Err(Failure::Failed(format!("running kubectl: {e}"))),
        Err(_) => return Err(Failure::TimedOut),
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let line = stderr
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or("kubectl get pods failed");
        return Err(Failure::Failed(line.to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A pod as one table row would show it.
struct Line<'a> {
    pod: &'a Pod,
    status: String,
    health: Health,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Health {
    /// Running and ready, or finished fine.
    Good,
    /// Starting, terminating or not ready yet.
    Pending,
    /// Failing.
    Bad,
}

impl Health {
    fn status(self) -> Status {
        match self {
            Health::Good => Status::Ok,
            Health::Pending => Status::Warn,
            Health::Bad => Status::Error,
        }
    }
}

/// The STATUS column of `kubectl get pods`, close enough for a tile.
fn pod_status(pod: &Pod) -> String {
    if pod.metadata.deletion_timestamp.is_some() {
        return "Terminating".into();
    }
    let s = &pod.status;
    for c in &s.init_container_statuses {
        if let Some(reason) = c.state.waiting.as_ref().and_then(|w| w.reason.clone())
            && reason != "PodInitializing"
        {
            return format!("Init:{reason}");
        }
        if let Some(t) = &c.state.terminated
            && t.reason.as_deref() != Some("Completed")
        {
            return format!("Init:{}", t.reason.clone().unwrap_or("Error".into()));
        }
    }
    for c in &s.container_statuses {
        if let Some(reason) = c.state.waiting.as_ref().and_then(|w| w.reason.clone()) {
            return reason;
        }
    }
    for c in &s.container_statuses {
        if let Some(reason) = c.state.terminated.as_ref().and_then(|t| t.reason.clone())
            && s.phase != "Succeeded"
        {
            return reason;
        }
    }
    match (&s.reason, s.phase.as_str()) {
        (Some(reason), _) => reason.clone(),
        (None, "Succeeded") => "Completed".into(),
        (None, "") => "Unknown".into(),
        (None, phase) => phase.into(),
    }
}

fn health(pod: &Pod, status: &str) -> Health {
    match status {
        "Running" => {
            if pod.status.container_statuses.iter().all(|c| c.ready) {
                Health::Good
            } else {
                Health::Pending
            }
        }
        "Completed" => Health::Good,
        "Pending" | "ContainerCreating" | "PodInitializing" | "Terminating" => Health::Pending,
        s if s.starts_with("Init:") && !s.contains("BackOff") && !s.contains("Error") => {
            Health::Pending
        }
        _ => Health::Bad,
    }
}

/// "45s", "12m", "3h", "9d", like kubectl's AGE column.
fn age(created: Option<&str>, now: i64) -> String {
    let Some(then) = created.and_then(parse_time) else {
        return "-".into();
    };
    match (now - then).max(0) {
        s if s < 120 => format!("{s}s"),
        s if s < 7_200 => format!("{}m", s / 60),
        s if s < 172_800 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn column(key: &str, label: &str) -> Column {
    Column {
        key: key.into(),
        label: label.into(),
    }
}

/// The tile: pods per namespace as bars, then one row per pod: failing pods
/// first, then pending, then the rest, each by namespace and name. `now` is unix seconds.
fn view(pods: &[Pod], now: i64, problems_only: bool, limit: usize) -> View {
    if pods.is_empty() {
        return View {
            status: Status::Ok,
            summary: "no pods".into(),
            root: Widget::Text {
                text: "No pods.".into(),
                markdown: false,
            },
        };
    }
    let mut lines: Vec<Line> = pods
        .iter()
        .map(|pod| {
            let status = pod_status(pod);
            let health = health(pod, &status);
            Line {
                pod,
                status,
                health,
            }
        })
        .collect();
    // Worst first across all namespaces, so problems survive `limit`; the
    // namespace bars give the grouping.
    lines.sort_by(|a, b| {
        (std::cmp::Reverse(a.health), &a.pod.metadata.namespace)
            .cmp(&(std::cmp::Reverse(b.health), &b.pod.metadata.namespace))
            .then_with(|| a.pod.metadata.name.cmp(&b.pod.metadata.name))
    });

    let bad = lines.iter().filter(|l| l.health == Health::Bad).count();
    let pending = lines.iter().filter(|l| l.health == Health::Pending).count();
    let mut summary = vec![match pods.len() {
        1 => "1 pod".to_string(),
        n => format!("{n} pods"),
    }];
    if bad > 0 {
        summary.push(format!("{bad} failing"));
    }
    if pending > 0 {
        summary.push(format!("{pending} pending"));
    }
    let status = if bad > 0 {
        Status::Error
    } else if pending > 0 {
        Status::Warn
    } else {
        Status::Ok
    };

    // Pods per namespace; the bar takes the worst pod's status.
    let mut namespaces: BTreeMap<&str, (usize, Health)> = BTreeMap::new();
    for l in &lines {
        let e = namespaces
            .entry(&l.pod.metadata.namespace)
            .or_insert((0, Health::Good));
        e.0 += 1;
        e.1 = e.1.max(l.health);
    }
    let bars = namespaces
        .iter()
        .map(|(ns, (count, worst))| Bar {
            label: ns.to_string(),
            value: *count as f64,
            status: Some(worst.status()),
        })
        .collect();

    let rows = lines
        .iter()
        .filter(|l| !problems_only || l.health != Health::Good)
        .take(limit)
        .map(|l| {
            let cs = &l.pod.status.container_statuses;
            let ready = cs.iter().filter(|c| c.ready).count();
            let restarts: u32 = cs.iter().map(|c| c.restart_count).sum();
            let cells = [
                ("namespace", l.pod.metadata.namespace.clone()),
                ("pod", l.pod.metadata.name.clone()),
                ("status", l.status.clone()),
                ("ready", format!("{ready}/{}", cs.len())),
                ("restarts", restarts.to_string()),
                (
                    "age",
                    age(l.pod.metadata.creation_timestamp.as_deref(), now),
                ),
            ];
            Row {
                cells: cells
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), Cell::Text(v)))
                    .collect(),
                status: Some(l.health.status()),
                action: None,
            }
        })
        .collect();

    View {
        status,
        summary: summary.join(" · "),
        root: Widget::Column {
            children: vec![
                Widget::Bars {
                    label: Some("Pods by namespace".into()),
                    unit: None,
                    format: ValueFormat::Plain,
                    max: None,
                    bars,
                },
                Widget::Table {
                    columns: vec![
                        column("namespace", "Namespace"),
                        column("pod", "Pod"),
                        column("status", "Status"),
                        column("ready", "Ready"),
                        column("restarts", "Restarts"),
                        column("age", "Age"),
                    ],
                    rows,
                },
            ],
        },
    }
}

const SAMPLE: &str = include_str!("../testdata/kubernetes/pods.json");
/// The sample's "now", so its ages read the same whenever the demo is shown.
const SAMPLE_NOW: &str = "2026-10-27T12:00:00Z";

pub fn catalog() -> CatalogEntry {
    let pods: PodList = serde_json::from_str(SAMPLE).expect("demo data parses");
    let now = parse_time(SAMPLE_NOW).expect("demo time parses");
    CatalogEntry {
        description: "Kubernetes pods by namespace with their status, ready containers, \
                      restarts and age, read with kubectl. Failing pods are listed first."
            .into(),
        tags: crate::strings(&["kubernetes", "k8s", "ops", "containers"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["kubectl"]),
            env: vec![],
            hosts: vec![],
        },
        ..crate::entry(
            "kubernetes",
            "Kubernetes",
            Size::Wide,
            view(&pods.items, now, false, 30),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(row: &Row, key: &str) -> String {
        match &row.cells[key] {
            Cell::Text(t) => t.clone(),
            other => panic!("{key}: {other:?}"),
        }
    }

    fn parts(v: &View) -> (&[Bar], &[Row]) {
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::Bars { bars, .. } = &children[0] else {
            panic!("bars")
        };
        let Widget::Table { rows, .. } = &children[1] else {
            panic!("table")
        };
        (bars, rows)
    }

    #[test]
    fn parses_the_sample() {
        let v = catalog().demo;
        assert_eq!(v.status, Status::Error);
        assert_eq!(v.summary, "7 pods · 2 failing · 2 pending");
        let (bars, rows) = parts(&v);
        let ns: Vec<(&str, f64, Option<Status>)> = bars
            .iter()
            .map(|b| (b.label.as_str(), b.value, b.status))
            .collect();
        assert_eq!(
            ns,
            [
                ("kube-system", 2.0, Some(Status::Warn)),
                ("shop", 3.0, Some(Status::Error)),
                ("staging", 2.0, Some(Status::Error)),
            ]
        );
        assert_eq!(rows.len(), 7);
        // Failing pods first, in any namespace.
        assert_eq!(text(&rows[0], "status"), "CrashLoopBackOff");
        assert_eq!(text(&rows[0], "namespace"), "shop");
        assert_eq!(text(&rows[0], "restarts"), "14");
        assert_eq!(text(&rows[0], "ready"), "0/1");
        assert_eq!(rows[0].status, Some(Status::Error));
        assert_eq!(text(&rows[1], "status"), "Init:ImagePullBackOff");
        // Then pending ones.
        assert_eq!(text(&rows[2], "status"), "ContainerCreating");
        assert_eq!(text(&rows[2], "age"), "90s");
        assert_eq!(text(&rows[3], "status"), "Terminating");
        assert_eq!(rows[3].status, Some(Status::Warn));
        // Then the healthy ones, by namespace and name.
        assert_eq!(text(&rows[4], "pod"), "coredns-76f75df574-8zq2w");
        assert_eq!(text(&rows[4], "age"), "56d");
        assert_eq!(text(&rows[5], "status"), "Completed");
        assert_eq!(rows[5].status, Some(Status::Ok));
        assert_eq!(text(&rows[6], "ready"), "2/2");
        assert_eq!(text(&rows[6], "restarts"), "1");
        assert_eq!(text(&rows[6], "age"), "2d");
    }

    #[test]
    fn problems_only_and_limit() {
        let pods: PodList = serde_json::from_str(SAMPLE).unwrap();
        let now = parse_time(SAMPLE_NOW).unwrap();
        let v = view(&pods.items, now, true, 30);
        let (bars, rows) = parts(&v);
        assert_eq!(bars.len(), 3);
        assert_eq!(rows.len(), 4);
        // The limit keeps the failing pods, whatever their namespace.
        let v = view(&pods.items, now, false, 2);
        let rows = parts(&v).1;
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.status == Some(Status::Error)));
        assert_eq!(view(&[], now, false, 30).summary, "no pods");
    }

    #[test]
    fn failures() {
        let v = Failure::Failed(
            "The connection to the server localhost:8080 was refused - did you specify the \
             right host or port?"
                .into(),
        )
        .view();
        assert_eq!(v.status, Status::Error);
        assert_eq!(v.summary, "cluster unreachable");
        assert_eq!(Failure::NotFound.view().summary, "kubectl not found");
    }

    #[tokio::test]
    async fn missing_kubectl_is_an_error_tile() {
        let mut m = Kubernetes::default();
        m.init(serde_json::json!({ "kubectl": "/nonexistent/kubectl" }))
            .await
            .unwrap();
        assert_eq!(m.render().await.unwrap().summary, "kubectl not found");
        assert!(
            Kubernetes::default()
                .init(serde_json::json!({ "namespce": "x" }))
                .await
                .is_err()
        );
        assert!(
            Kubernetes::default()
                .init(serde_json::json!({ "limit": 0 }))
                .await
                .is_err()
        );
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
