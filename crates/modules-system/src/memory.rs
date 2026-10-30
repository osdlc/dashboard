//! Memory and swap: usage graph and gauges.

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, ValueFormat, View, Widget};
use dashboard_host::{Module, now_ms};
use sysinfo::System;

use super::{
    demo_graph, demo_series, demo_values, gauge, graph, manifest, percent, sample, status_for,
    used_of,
};

#[derive(Default)]
pub struct Memory {
    sys: System,
}

#[async_trait]
impl Module for Memory {
    async fn init(&mut self, _config: serde_json::Value) -> Result<Manifest> {
        Ok(manifest("memory", "Memory", 1000, 250, Size::Medium, 90))
    }

    async fn render(&mut self) -> Result<View> {
        self.sys.refresh_memory();
        let s = &self.sys;
        let mem = percent(s.used_memory(), s.total_memory());
        let swap = percent(s.used_swap(), s.total_swap());
        Ok(View {
            status: status_for(mem),
            summary: format!("{mem:.0}% · swap {swap:.0}%"),
            root: Widget::Column {
                children: vec![
                    graph(
                        "used",
                        "Used",
                        "%",
                        ValueFormat::Plain,
                        Some(100.0),
                        vec![sample("used", now_ms(), mem)],
                    ),
                    gauge("Memory", mem, used_of(s.used_memory(), s.total_memory())),
                    gauge("Swap", swap, used_of(s.used_swap(), s.total_swap())),
                    Widget::Text {
                        text: format!("available {}", super::bytes(s.available_memory() as f64)),
                        markdown: false,
                    },
                ],
            },
        })
    }
}

pub fn catalog() -> CatalogEntry {
    const GIB: u64 = 1024 * 1024 * 1024;
    let used = demo_values(60, 62.0, 6.0, 2.0);
    let demo = View {
        status: Status::Ok,
        summary: "62% · swap 18%".into(),
        root: Widget::Column {
            children: vec![
                demo_graph(
                    "Used",
                    "%",
                    ValueFormat::Plain,
                    Some(100.0),
                    vec![demo_series("used", 1000, &used)],
                ),
                gauge("Memory", 62.0, used_of(20 * GIB, 32 * GIB)),
                gauge("Swap", 18.0, used_of(GIB * 18 / 25, 4 * GIB)),
                Widget::Text {
                    text: format!("available {}", super::bytes((12 * GIB) as f64)),
                    markdown: false,
                },
            ],
        },
    };
    CatalogEntry {
        description: "Memory and swap use: a graph over time and a gauge for each.".into(),
        tags: crate::strings(&["system", "local"]),
        ..crate::entry("memory", "Memory", Size::Medium, demo)
    }
}
