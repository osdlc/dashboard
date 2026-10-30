//! CPU: total usage graph, per-core meters, load average.

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, ColorScale, Manifest, Meter, Size, Status, ValueFormat, View, Widget,
};
use dashboard_host::{Module, now_ms};
use sysinfo::System;

use super::{demo_graph, demo_series, demo_values, graph, manifest, sample, status_for};

#[derive(Default)]
pub struct Cpu {
    sys: System,
}

#[async_trait]
impl Module for Cpu {
    async fn init(&mut self, _config: serde_json::Value) -> Result<Manifest> {
        Ok(manifest("cpu", "CPU", 500, 250, Size::Wide, 100))
    }

    async fn render(&mut self) -> Result<View> {
        self.sys.refresh_cpu_usage();
        let total = self.sys.global_cpu_usage() as f64;
        let cores = self.sys.cpus();
        let load = System::load_average();
        let load_status = if load.one > cores.len().max(1) as f64 {
            Status::Warn
        } else {
            Status::Ok
        };

        let meters = cores
            .iter()
            .enumerate()
            .map(|(i, c)| Meter {
                label: format!("{}", i + 1),
                percent: c.cpu_usage() as f64,
            })
            .collect();
        Ok(View {
            status: status_for(total).max_with(load_status),
            summary: format!("{total:.0}% · load {:.2}", load.one),
            root: Widget::Column {
                children: vec![
                    graph(
                        "total",
                        "Total",
                        "%",
                        ValueFormat::Plain,
                        Some(100.0),
                        vec![sample("total", now_ms(), total)],
                    ),
                    Widget::MeterGrid {
                        label: None,
                        meters,
                        color: ColorScale::Heat,
                    },
                    Widget::Text {
                        text: format!(
                            "load  {:.2}  {:.2}  {:.2}   ({} cores)",
                            load.one,
                            load.five,
                            load.fifteen,
                            cores.len()
                        ),
                        markdown: false,
                    },
                ],
            },
        })
    }
}

pub fn catalog() -> CatalogEntry {
    let total = demo_values(120, 34.0, 20.0, 1.0);
    let cores = [41.0, 27.5, 63.0, 18.2, 35.9, 22.4, 12.0, 8.7];
    let demo = View {
        status: Status::Ok,
        summary: format!("{:.0}% · load 2.31", total[total.len() - 1]),
        root: Widget::Column {
            children: vec![
                demo_graph(
                    "Total",
                    "%",
                    ValueFormat::Plain,
                    Some(100.0),
                    vec![demo_series("total", 500, &total)],
                ),
                Widget::MeterGrid {
                    label: None,
                    meters: cores
                        .iter()
                        .enumerate()
                        .map(|(i, p)| Meter {
                            label: format!("{}", i + 1),
                            percent: *p,
                        })
                        .collect(),
                    color: ColorScale::Heat,
                },
                Widget::Text {
                    text: "load  2.31  2.05  1.88   (8 cores)".into(),
                    markdown: false,
                },
            ],
        },
    };
    CatalogEntry {
        description: "Total CPU use over time, a meter per core, and the load average.".into(),
        tags: crate::strings(&["system", "local"]),
        ..crate::entry("cpu", "CPU", Size::Wide, demo)
    }
}

trait MaxWith {
    fn max_with(self, other: Status) -> Status;
}

impl MaxWith for Status {
    /// The more severe of two statuses.
    fn max_with(self, other: Status) -> Status {
        let rank = |s: Status| match s {
            Status::Unknown | Status::Ok => 0,
            Status::Warn => 1,
            Status::Error => 2,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}
