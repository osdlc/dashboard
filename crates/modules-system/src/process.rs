//! One process and everything it started, as three numbers: CPU, memory and
//! the number of processes. With no settings it watches sitroom itself,
//! so the tile shows what sitroom and its plugins cost:
//!
//! ```yaml
//! modules:
//!   process: {}            # sitroom and its plugins
//!   postgres:
//!     builtin: process
//!     config:
//!       name: postgres     # every process with this name, and their children
//!       children: true     # false: leave the children out
//! ```

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use sitroom_contract::{CatalogEntry, Manifest, Size, Status, View, Widget};
use sitroom_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::{bytes, manifest};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Watch every process with this name, e.g. `postgres`. Default: the
    /// sitroom itself.
    name: Option<String>,
    /// Count the processes they started too (for sitroom: its plugins).
    children: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: None,
            children: true,
        }
    }
}

/// How often every process is listed to find the watched ones. In between,
/// only the watched ones are read, so the tile costs little itself. A process
/// started since the last listing is counted from the next one.
const LIST_EVERY: Duration = Duration::from_secs(10);

#[derive(Default)]
pub struct Process {
    config: Config,
    sys: System,
    /// The watched processes, as of `listed`.
    watched: Vec<Pid>,
    listed: Option<Instant>,
}

/// What the module reads of one running process.
#[derive(Debug, Clone, PartialEq)]
struct Proc {
    pid: u32,
    parent: Option<u32>,
    name: String,
    /// Percent of one core, so several busy threads can pass 100.
    cpu: f64,
    /// Resident memory, in bytes.
    memory: u64,
}

#[derive(Debug, Default, PartialEq)]
struct Total {
    count: usize,
    cpu: f64,
    memory: u64,
}

#[async_trait]
impl Module for Process {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let title = self.config.name.as_deref().unwrap_or("Sitroom");
        Ok(manifest("process", title, 2000, 1000, Size::Small, 60))
    }

    async fn render(&mut self) -> Result<View> {
        if self.listed.is_none_or(|t| t.elapsed() >= LIST_EVERY) {
            self.list();
            self.listed = Some(Instant::now());
        }
        // The first reading of a process has no CPU figure yet.
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&self.watched),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let procs: Vec<Proc> = self
            .watched
            .iter()
            .filter_map(|pid| self.sys.process(*pid))
            .map(read)
            .collect();
        Ok(view(sum(&procs), self.config.name.as_deref()))
    }
}

impl Process {
    /// Finds the watched processes among all running ones: a child is only
    /// found through its parent's ID, so every process is listed.
    fn list(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let procs: Vec<Proc> = self
            .sys
            .processes()
            .values()
            // Linux lists threads too; their memory is the process's own.
            .filter(|p| p.thread_kind().is_none())
            .map(read)
            .collect();
        let roots: Vec<u32> = match &self.config.name {
            Some(name) => procs
                .iter()
                .filter(|p| p.name == *name)
                .map(|p| p.pid)
                .collect(),
            None => vec![std::process::id()],
        };
        self.watched = members(&procs, &roots, self.config.children)
            .into_iter()
            .map(Pid::from_u32)
            .collect();
    }
}

fn read(p: &sysinfo::Process) -> Proc {
    Proc {
        pid: p.pid().as_u32(),
        parent: p.parent().map(|pid| pid.as_u32()),
        name: p.name().to_string_lossy().into_owned(),
        cpu: p.cpu_usage() as f64,
        memory: p.memory(),
    }
}

/// `roots` and, with `children`, everything below them.
fn members(procs: &[Proc], roots: &[u32], children: bool) -> HashSet<u32> {
    let mut below: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        if let Some(parent) = p.parent {
            below.entry(parent).or_default().push(p.pid);
        }
    }
    let known: HashSet<u32> = procs.iter().map(|p| p.pid).collect();
    let mut picked: HashSet<u32> = HashSet::new();
    let mut todo: Vec<u32> = roots
        .iter()
        .copied()
        .filter(|r| known.contains(r))
        .collect();
    while let Some(pid) = todo.pop() {
        if picked.insert(pid) && children {
            todo.extend(below.get(&pid).into_iter().flatten());
        }
    }
    picked
}

fn sum(procs: &[Proc]) -> Total {
    let mut sum = Total::default();
    for p in procs {
        sum.count += 1;
        sum.cpu += p.cpu;
        sum.memory += p.memory;
    }
    sum
}

fn view(sum: Total, name: Option<&str>) -> View {
    if sum.count == 0 {
        return View {
            status: Status::Unknown,
            summary: "not running".into(),
            root: Widget::Text {
                text: format!("{} is not running", name.unwrap_or("The process")),
                markdown: false,
            },
        };
    }
    let memory = bytes(sum.memory as f64);
    let stat = |label: &str, value: String, unit: Option<&str>| Widget::Stat {
        label: label.into(),
        value,
        unit: unit.map(Into::into),
        delta: None,
        status: None,
    };
    View {
        status: Status::Ok,
        summary: format!("{:.1}% CPU · {memory} · {} proc", sum.cpu, sum.count),
        root: Widget::Row {
            children: vec![
                stat("CPU", format!("{:.1}", sum.cpu), Some("%")),
                stat("Memory", memory.clone(), None),
                stat("Processes", sum.count.to_string(), None),
            ],
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let demo = view(
        Total {
            count: 4,
            cpu: 1.8,
            memory: 96 * 1024 * 1024,
        },
        None,
    );
    CatalogEntry {
        description: "CPU and memory of one process and the processes it started, as three \
                      numbers. With no settings it watches sitroom itself and its plugins. \
                      Add it several times with `builtin: process`."
            .into(),
        tags: crate::strings(&["system", "local", "process"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("process", "Sitroom", Size::Small, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: Option<u32>, name: &str, cpu: f64, memory: u64) -> Proc {
        Proc {
            pid,
            parent,
            name: name.into(),
            cpu,
            memory,
        }
    }

    /// Adds up `roots` and what is below them. A process under two roots is
    /// counted once.
    fn total(procs: &[Proc], roots: &[u32], children: bool) -> Total {
        let picked = members(procs, roots, children);
        let picked: Vec<Proc> = procs
            .iter()
            .filter(|p| picked.contains(&p.pid))
            .cloned()
            .collect();
        sum(&picked)
    }

    /// 10 → 11 → 12, with 20 unrelated.
    fn sample() -> Vec<Proc> {
        vec![
            proc(1, None, "init", 0.5, 1),
            proc(10, Some(1), "sitroom", 1.0, 100),
            proc(11, Some(10), "node", 2.0, 200),
            proc(12, Some(11), "gh", 4.0, 400),
            proc(20, Some(1), "other", 50.0, 5000),
        ]
    }

    #[test]
    fn adds_up_the_whole_tree() {
        assert_eq!(
            total(&sample(), &[10], true),
            Total {
                count: 3,
                cpu: 7.0,
                memory: 700
            }
        );
    }

    #[test]
    fn without_children_only_the_roots_count() {
        assert_eq!(
            total(&sample(), &[10], false),
            Total {
                count: 1,
                cpu: 1.0,
                memory: 100
            }
        );
    }

    #[test]
    fn a_process_under_two_roots_counts_once() {
        assert_eq!(total(&sample(), &[10, 11], true).count, 3);
    }

    #[test]
    fn a_parent_loop_ends() {
        let procs = vec![proc(1, Some(2), "a", 1.0, 1), proc(2, Some(1), "b", 1.0, 1)];
        assert_eq!(total(&procs, &[1], true).count, 2);
    }

    #[test]
    fn a_missing_root_is_not_running() {
        let sum = total(&sample(), &[99], true);
        assert_eq!(sum, Total::default());
        let v = view(sum, Some("postgres"));
        assert_eq!(v.status, Status::Unknown);
        assert_eq!(v.summary, "not running");
    }

    #[test]
    fn the_view_is_three_stats() {
        let v = view(total(&sample(), &[10], true), None);
        assert_eq!(v.summary, "7.0% CPU · 700 B · 3 proc");
        let Widget::Row { children } = v.root else {
            panic!("expected a row");
        };
        assert_eq!(children.len(), 3);
    }

    #[tokio::test]
    async fn watches_this_process_by_default() {
        let mut m = Process::default();
        m.init(serde_json::Value::Null).await.unwrap();
        let v = m.render().await.unwrap();
        assert_eq!(v.status, Status::Ok, "{}", v.summary);
    }

    #[tokio::test]
    async fn unknown_settings_are_refused() {
        let mut m = Process::default();
        assert!(m.init(serde_json::json!({ "pid": 1 })).await.is_err());
    }
}
