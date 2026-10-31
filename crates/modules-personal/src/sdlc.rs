//! The backlog of an osdlc sdlc vault: tasks in progress and ready to pick
//! up (open tasks whose `depends_on` are not closed are listed as waiting),
//! and how far the open milestones have come. Reads the markdown
//! entities under `docs/planning/` (`tasks/T-*.md`, `milestones/M-*.md`) and
//! their YAML front matter. It only reads; the `sdlc` CLI is not needed.
//!
//! ```yaml
//! modules:
//!   sdlc:
//!     config:
//!       path: ~/code/myproject     # the repo, or its docs/planning folder
//!       link: "https://github.com/me/myproject/blob/main/{path}"
//! ```

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use jiff::civil::Date;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{day_text, expand_home, link, manifest, parse_date, today};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The repository, or its `docs/planning` folder. `~/` is expanded.
    #[schemars(extend("x-path" = true))]
    path: String,
    /// Also list tasks still in planning (`planning/*` states).
    planning: bool,
    /// Rows shown.
    limit: usize,
    /// Open milestones shown as progress bars, active ones first.
    milestones: usize,
    /// Makes each task a link: `{path}` is the file's path under `path`,
    /// e.g. `https://github.com/me/repo/blob/main/{path}`. Without it, a
    /// task in progress links to its last pull request.
    link: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: ".".into(),
            planning: false,
            limit: 20,
            milestones: 2,
            link: None,
        }
    }
}

/// A task or milestone file.
#[derive(Debug, Clone, Default, PartialEq)]
struct Entity {
    id: String,
    state: String,
    title: String,
    impact: Option<String>,
    priority: bool,
    created: Option<String>,
    tags: Vec<String>,
    prs: Vec<String>,
    /// Milestones: the release version, target date and member ids.
    version: Option<String>,
    target_date: Option<Date>,
    members: Vec<String>,
    /// Tasks: the ids this one waits on (`depends_on`).
    depends_on: Vec<String>,
    action: Option<Action>,
}

impl Entity {
    fn closed(&self) -> bool {
        self.state.starts_with("closed")
    }
}

#[derive(Default)]
pub struct Sdlc {
    config: Config,
}

#[async_trait]
impl Module for Sdlc {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest("sdlc", "Backlog", 60_000, 5_000, Size::Medium, 20))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let (tasks, milestones) = load(&expand_home(&c.path), c.link.as_deref())?;
        Ok(view(&tasks, &milestones, today(), c))
    }
}

/// The planning folder: `<path>/docs/planning`, or `path` itself.
fn planning_dir(path: &Path) -> Result<PathBuf> {
    let nested = path.join("docs").join("planning");
    if nested.join("tasks").is_dir() {
        return Ok(nested);
    }
    if path.join("tasks").is_dir() {
        return Ok(path.to_path_buf());
    }
    bail!(
        "no sdlc vault at {}: expected docs/planning/tasks in it",
        path.display()
    )
}

/// Every task and milestone under `root`.
fn load(root: &Path, template: Option<&str>) -> Result<(Vec<Entity>, Vec<Entity>)> {
    let dir = planning_dir(root)?;
    let read = |kind: &str, prefix: &str| -> Result<Vec<Entity>> {
        let folder = dir.join(kind);
        let Ok(entries) = std::fs::read_dir(&folder) else {
            return Ok(vec![]);
        };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        let mut out = vec![];
        for file in files {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            if !name.starts_with(prefix) || !name.ends_with(".md") {
                continue;
            }
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            if let Some(mut e) = entity(&text, &name) {
                e.action = link(template, root, &file, 1);
                out.push(e);
            }
        }
        Ok(out)
    };
    Ok((read("tasks", "T-")?, read("milestones", "M-")?))
}

/// An entity from a file's text. Files without front matter are skipped.
fn entity(text: &str, file_name: &str) -> Option<Entity> {
    let rest = text.strip_prefix("---")?.trim_start_matches([' ', '\t']);
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---")?;
    let (front, body) = (&rest[..end], &rest[end + 4..]);
    let front: Value = serde_saphyr::from_str(front).ok()?;
    let s = |key: &str| match front.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let list = |key: &str| -> Vec<String> {
        front
            .get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    // The id from the front matter, else from the file name (`T-AAAA-slug.md`).
    let id = s("id").unwrap_or_else(|| wikilink_id(file_name.trim_end_matches(".md")));
    let title = s("title")
        .or_else(|| {
            body.lines()
                .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        })
        .unwrap_or_else(|| id.clone());
    Some(Entity {
        state: s("state").unwrap_or_else(|| "planning/draft".into()),
        title,
        impact: s("impact"),
        priority: front
            .get("priority")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        created: s("created"),
        tags: list("tags"),
        prs: list("prs"),
        version: s("version"),
        target_date: s("target_date").as_deref().and_then(parse_date),
        members: list("tasks").iter().map(|l| wikilink_id(l)).collect(),
        depends_on: list("depends_on").iter().map(|l| wikilink_id(l)).collect(),
        id,
        action: None,
    })
}

/// `[[T-AAAA-alpha-task]]` → `T-AAAA`; `M-0001.1-x` → `M-0001.1`.
fn wikilink_id(link: &str) -> String {
    let inner = link.trim_start_matches("[[").trim_end_matches("]]");
    let inner = inner.split(['|', '#']).next().unwrap_or(inner);
    let mut parts = inner.splitn(3, '-');
    match (parts.next(), parts.next()) {
        (Some(kind), Some(n)) => format!("{kind}-{n}"),
        _ => inner.to_string(),
    }
}

/// Where a task stands, in the order the tile lists them. An open task
/// whose dependencies are not all closed is waiting, not ready.
fn stage(state: &str, waiting: bool) -> Option<(u8, &'static str)> {
    match state {
        "in-progress/blocked" => Some((0, "blocked")),
        s if s.starts_with("in-progress") => Some((1, "in progress")),
        s if s.starts_with("open") && !waiting => Some((2, "ready")),
        s if s.starts_with("open") => Some((3, "waiting")),
        s if s.starts_with("planning") => Some((4, "planning")),
        _ => None,
    }
}

fn impact_rank(impact: Option<&str>) -> u8 {
    match impact {
        Some("high") => 0,
        Some("low") => 2,
        _ => 1,
    }
}

/// `1.10.0` sorts after `1.9.0`; a milestone without a version sorts last.
fn version_key(v: Option<&str>) -> (bool, Vec<u64>, String) {
    match v {
        Some(v) => (
            false,
            v.split(['.', '-'])
                .map_while(|p| p.trim_start_matches('v').parse().ok())
                .collect(),
            v.to_string(),
        ),
        None => (true, vec![], String::new()),
    }
}

fn view(tasks: &[Entity], milestones: &[Entity], today: Date, c: &Config) -> View {
    // Dependencies that are still open. One not in the vault does not count.
    let open_deps = |t: &Entity| -> Vec<String> {
        t.depends_on
            .iter()
            .filter(|d| tasks.iter().any(|o| &o.id == *d && !o.closed()))
            .cloned()
            .collect()
    };
    let mut open: Vec<(&Entity, u8, &str)> = tasks
        .iter()
        .filter_map(|t| {
            stage(&t.state, !open_deps(t).is_empty()).map(|(rank, label)| (t, rank, label))
        })
        .filter(|(_, rank, _)| *rank < 4 || c.planning)
        .collect();
    open.sort_by_key(|(t, rank, _)| {
        (
            *rank,
            Reverse(t.priority),
            impact_rank(t.impact.as_deref()),
            t.created.clone(),
            t.id.clone(),
        )
    });

    // Open milestones: active first, then by version.
    let mut ms: Vec<&Entity> = milestones.iter().filter(|m| !m.closed()).collect();
    ms.sort_by_key(|m| {
        (
            m.state != "open/active",
            version_key(m.version.as_deref()),
            m.id.clone(),
        )
    });
    let milestone_of = |task: &Entity| {
        ms.iter()
            .find(|m| m.members.contains(&task.id))
            .map(|m| m.id.clone())
    };

    let mut children: Vec<Widget> = ms
        .iter()
        .take(c.milestones)
        .map(|m| {
            let members: Vec<&Entity> =
                tasks.iter().filter(|t| m.members.contains(&t.id)).collect();
            let done = members.iter().filter(|t| t.closed()).count();
            let percent = if members.is_empty() {
                0.0
            } else {
                done as f64 * 100.0 / members.len() as f64
            };
            let mut caption = vec![format!("{done}/{} done", members.len())];
            caption.extend(
                m.version
                    .as_ref()
                    .map(|v| format!("v{}", v.trim_start_matches('v'))),
            );
            caption.extend(
                m.target_date
                    .map(|d| format!("target {}", day_text(d, today))),
            );
            Widget::Gauge {
                label: format!("{} {}", m.id, m.title),
                percent,
                marker: None,
                thresholds: None,
                caption: Some(caption.join(" · ")),
            }
        })
        .collect();

    let items = open
        .iter()
        .take(c.limit)
        .map(|(t, rank, label)| {
            let mut sub = vec![label.to_string()];
            if *rank == 3 {
                sub.push(format!("on {}", open_deps(t).join(", ")));
            }
            if t.priority {
                sub.push("priority".into());
            }
            sub.extend(t.impact.as_ref().map(|i| format!("{i} impact")));
            sub.extend(milestone_of(t));
            let mut tags = vec![t.id.clone()];
            tags.extend(t.tags.iter().cloned());
            let pr = (*rank < 2)
                .then(|| t.prs.last())
                .flatten()
                .map(|url| Action::OpenUrl { url: url.clone() });
            ListItem {
                tone: None,
                title: t.title.clone(),
                subtitle: Some(sub.join(" · ")),
                status: match rank {
                    0 => Some(Status::Warn),
                    1 => Some(Status::Ok),
                    _ => None,
                },
                tags,
                action: t.action.clone().or(pr),
            }
        })
        .collect();
    children.push(Widget::List {
        items,
        empty_text: Some("nothing open".into()),
    });

    let count = |r: u8| open.iter().filter(|(_, rank, _)| *rank == r).count();
    let (blocked, doing, waiting, ready) = (count(0), count(1), count(3), count(2));
    let mut summary = vec![];
    if doing > 0 {
        summary.push(format!("{doing} in progress"));
    }
    if blocked > 0 {
        summary.push(format!("{blocked} blocked"));
    }
    summary.push(format!("{ready} ready"));
    if waiting > 0 {
        summary.push(format!("{waiting} waiting"));
    }
    View {
        status: if blocked > 0 {
            Status::Warn
        } else {
            Status::Ok
        },
        summary: summary.join(" · "),
        root: Widget::Column { children },
    }
}

pub fn catalog() -> CatalogEntry {
    let t = |id: &str, state: &str, title: &str, impact: &str| Entity {
        id: id.into(),
        state: state.into(),
        title: title.into(),
        impact: Some(impact.into()),
        ..Entity::default()
    };
    let tasks = vec![
        t(
            "T-4KQ2",
            "in-progress",
            "Read config layers from the project folder",
            "high",
        ),
        t(
            "T-7HD1",
            "in-progress/blocked",
            "Sign the macOS build",
            "medium",
        ),
        Entity {
            priority: true,
            ..t(
                "T-2MZP",
                "open/ready",
                "Fix the reload race in the host",
                "high",
            )
        },
        t(
            "T-9WBC",
            "open/ready",
            "Add CalDAV to the calendar widget",
            "medium",
        ),
        t("T-1RTX", "open/ready", "Write the plugin SDK guide", "low"),
        t(
            "T-0AAA",
            "closed/done",
            "Split modules into bundles",
            "high",
        ),
        t(
            "T-0BBB",
            "closed/done",
            "Demo views in the catalog",
            "medium",
        ),
    ];
    let milestones = vec![Entity {
        id: "M-0003".into(),
        state: "open/active".into(),
        title: "Widget ecosystem".into(),
        version: Some("0.3.0".into()),
        members: crate::strings(&["T-4KQ2", "T-7HD1", "T-2MZP", "T-0AAA", "T-0BBB"]),
        ..Entity::default()
    }];
    let demo = view(&tasks, &milestones, today(), &Config::default());
    CatalogEntry {
        description: "The backlog of an osdlc sdlc vault: tasks in progress, blocked and \
                      ready, and how far the open milestones have come. Reads the markdown \
                      entities in docs/planning; read-only."
            .into(),
        tags: crate::strings(&["tasks", "sdlc", "osdlc", "project", "personal"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry("sdlc", "Backlog", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn vault() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/sdlc")
    }

    #[test]
    fn reads_a_vault() {
        let (tasks, milestones) =
            load(&vault(), Some("https://example.com/repo/blob/main/{path}")).unwrap();
        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "T-0D0N", "T-2BLK", "T-3CFG", "T-4PRI", "T-5LOW", "T-6PLN", "T-7CAL"
            ]
        );
        let doing = tasks.iter().find(|t| t.id == "T-2BLK").unwrap();
        assert_eq!(doing.title, "Sign the macOS build");
        // No title in the front matter: the heading.
        assert_eq!(tasks[2].title, "Read config layers from the project");
        assert_eq!(
            tasks[0].action,
            Some(Action::OpenUrl {
                url: "https://example.com/repo/blob/main/docs/planning/tasks/T-0D0N-bundles.md"
                    .into()
            })
        );
        assert_eq!(milestones.len(), 3);
        let m = &milestones[0];
        assert_eq!(m.id, "M-0001");
        assert_eq!(m.members, ["T-0D0N", "T-2BLK", "T-3CFG", "T-4PRI"]);
        assert_eq!(m.target_date, Some(date(2026, 11, 15)));
        // A bare-number version is read as text.
        assert_eq!(milestones[1].version.as_deref(), Some("1.0"));
    }

    #[test]
    fn backlog_view() {
        let (tasks, milestones) = load(&vault(), None).unwrap();
        let v = view(&tasks, &milestones, date(2026, 10, 27), &Config::default());
        assert_eq!(v.summary, "1 in progress · 1 blocked · 2 ready · 1 waiting");
        assert_eq!(v.status, Status::Warn);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        // The active milestone first; the closed one is left out.
        assert_eq!(children.len(), 3);
        let Widget::Gauge {
            label,
            percent,
            caption,
            ..
        } = &children[0]
        else {
            panic!()
        };
        assert_eq!(label, "M-0001 Widget ecosystem");
        assert_eq!(*percent, 25.0);
        assert_eq!(
            caption.as_deref(),
            Some("1/4 done · v0.3.0 · target 15 Nov")
        );
        let Widget::List { items, .. } = &children[2] else {
            panic!()
        };
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Sign the macOS build",
                "Read config layers from the project",
                "Fix the reload race",
                "Add CalDAV to the calendar",
                "Write the plugin SDK guide",
            ]
        );
        assert_eq!(items[0].status, Some(Status::Warn));
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("blocked · medium impact · M-0001")
        );
        assert_eq!(items[0].tags, ["T-2BLK", "release"]);
        assert_eq!(items[1].status, Some(Status::Ok));
        // In progress, no link template: its last PR.
        assert_eq!(
            items[1].action,
            Some(Action::OpenUrl {
                url: "https://github.com/acme/dash/pull/42".into()
            })
        );
        assert_eq!(
            items[2].subtitle.as_deref(),
            Some("ready · priority · low impact · M-0001")
        );
        // Open, but its dependency is still in progress.
        assert_eq!(
            items[4].subtitle.as_deref(),
            Some("waiting · on T-3CFG · low impact · M-0002")
        );
        assert_eq!(items[4].status, None);

        let c = Config {
            planning: true,
            milestones: 1,
            ..Config::default()
        };
        let v = view(&tasks, &milestones, date(2026, 10, 27), &c);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        assert_eq!(children.len(), 2);
        let Widget::List { items, .. } = &children[1] else {
            panic!()
        };
        assert_eq!(
            items.last().unwrap().subtitle.as_deref(),
            Some("planning · medium impact")
        );
    }

    #[test]
    fn ids_and_versions() {
        assert_eq!(wikilink_id("[[T-AAAA-alpha-task]]"), "T-AAAA");
        assert_eq!(wikilink_id("[[T-AAAA]]"), "T-AAAA");
        assert_eq!(wikilink_id("[[M-0001.1-sub|Sub]]"), "M-0001.1");
        assert!(version_key(Some("1.10.0")) > version_key(Some("1.9.0")));
        assert!(version_key(None) > version_key(Some("9.0.0")));
        assert!(entity("no front matter", "T-1.md").is_none());
        assert!(planning_dir(Path::new("/nonexistent")).is_err());
    }
}
