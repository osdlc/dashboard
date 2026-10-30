//! Network: download/upload graph (all interfaces except loopback).

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, Manifest, SeriesData, Size, Status, ValueFormat, View, format_value,
};
use dashboard_host::{Module, now_ms};
use sysinfo::Networks;

use super::{Rate, demo_graph, demo_series, demo_values, graph, manifest};

pub struct Network {
    networks: Networks,
    rate: Rate,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            networks: Networks::new_with_refreshed_list(),
            rate: Rate::default(),
        }
    }
}

#[async_trait]
impl Module for Network {
    async fn init(&mut self, _config: serde_json::Value) -> Result<Manifest> {
        Ok(manifest("network", "Network", 500, 250, Size::Medium, 80))
    }

    async fn render(&mut self) -> Result<View> {
        self.networks.refresh(true);
        let (rx, tx) = self
            .networks
            .iter()
            .filter(|(name, _)| !name.starts_with("lo"))
            .fold((0u64, 0u64), |(r, t), (_, n)| {
                (r + n.received(), t + n.transmitted())
            });
        let now = now_ms();
        let (series, summary) = match self.rate.elapsed() {
            Some(secs) => {
                let (down, up) = (rx as f64 / secs, tx as f64 / secs);
                let fmt = |v| format_value(v, ValueFormat::Bytes, Some("/s"));
                (
                    vec![
                        super::sample("down", now, down),
                        super::sample("up", now, up),
                    ],
                    format!("↓{} ↑{}", fmt(down), fmt(up)),
                )
            }
            // First refresh: no baseline for a rate yet.
            None => (
                vec![
                    SeriesData {
                        name: "down".into(),
                        points: vec![],
                    },
                    SeriesData {
                        name: "up".into(),
                        points: vec![],
                    },
                ],
                "measuring…".into(),
            ),
        };
        Ok(View {
            status: Status::Ok,
            summary,
            root: graph("traffic", "Traffic", "/s", ValueFormat::Bytes, None, series),
        })
    }
}

pub fn catalog() -> CatalogEntry {
    let down = demo_values(120, 1_400_000.0, 1_200_000.0, 0.5);
    let up = demo_values(120, 180_000.0, 150_000.0, 3.0);
    let fmt = |v: f64| format_value(v, ValueFormat::Bytes, Some("/s"));
    let demo = View {
        status: Status::Ok,
        summary: format!("↓{} ↑{}", fmt(down[119]), fmt(up[119])),
        root: demo_graph(
            "Traffic",
            "/s",
            ValueFormat::Bytes,
            None,
            vec![demo_series("down", 500, &down), demo_series("up", 500, &up)],
        ),
    };
    CatalogEntry {
        description: "Download and upload speed across all network interfaces.".into(),
        tags: crate::strings(&["system", "local", "network"]),
        ..crate::entry("network", "Network", Size::Medium, demo)
    }
}
