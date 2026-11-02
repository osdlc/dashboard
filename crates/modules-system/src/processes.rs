//! Top processes by CPU.

use std::collections::BTreeMap;

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Cell, Column, Manifest, Row, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::{bytes, manifest};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Rows to show.
    rows: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { rows: 12 }
    }
}

#[derive(Default)]
pub struct Processes {
    config: Config,
    sys: System,
}

#[async_trait]
impl Module for Processes {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest(
            "processes",
            "Processes",
            2000,
            1000,
            Size::Wide,
            60,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let mut procs: Vec<_> = self.sys.processes().values().collect();
        procs.sort_by(|a, b| b.cpu_usage().total_cmp(&a.cpu_usage()));

        let rows = procs
            .iter()
            .take(self.config.rows)
            .map(|p| {
                row(
                    &p.name().to_string_lossy(),
                    &p.pid().to_string(),
                    p.cpu_usage() as f64,
                    p.memory(),
                )
            })
            .collect();
        Ok(table(procs.len(), rows))
    }
}

fn table(count: usize, rows: Vec<Row>) -> View {
    let columns = [
        ("name", "Process"),
        ("pid", "PID"),
        ("cpu", "CPU %"),
        ("mem", "Memory"),
    ]
    .map(|(key, label)| Column {
        key: key.into(),
        label: label.into(),
        priority: None,
    })
    .to_vec();
    View {
        status: Status::Ok,
        summary: format!("{count} processes"),
        root: Widget::Table { columns, rows },
    }
}

fn row(name: &str, pid: &str, cpu: f64, memory: u64) -> Row {
    let cells = BTreeMap::from([
        ("name".to_string(), Cell::Text(name.into())),
        ("pid".to_string(), Cell::Text(pid.into())),
        ("cpu".to_string(), Cell::Text(format!("{cpu:.1}"))),
        ("mem".to_string(), Cell::Text(bytes(memory as f64))),
    ]);
    // CPU % is per core, so a busy process can exceed 100.
    let status = (cpu >= 100.0).then_some(Status::Warn);
    Row {
        cells,
        status,
        action: None,
    }
}

pub fn catalog() -> CatalogEntry {
    const MIB: u64 = 1024 * 1024;
    let rows = [
        ("rustc", "48211", 142.3, 1_210 * MIB),
        ("node", "3307", 38.6, 512 * MIB),
        ("Safari", "991", 21.4, 1_830 * MIB),
        ("postgres", "640", 9.8, 230 * MIB),
        ("Slack Helper", "2214", 6.1, 410 * MIB),
        ("kernel_task", "0", 4.7, 96 * MIB),
        ("dashboard", "51007", 1.2, 24 * MIB),
    ]
    .map(|(name, pid, cpu, mem)| row(name, pid, cpu, mem))
    .to_vec();
    CatalogEntry {
        description: "The busiest processes by CPU, with their memory use.".into(),
        tags: crate::strings(&["system", "local"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("processes", "Processes", Size::Wide, table(612, rows))
    }
}
