//! Network: download/upload graph (physical interfaces: not loopback, VPN
//! tunnels or container bridges, whose traffic also crosses a physical one).

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

/// Interfaces whose traffic is counted again on a physical one: loopback, VPN
/// tunnels (utun, wg, tailscale), and bridges and veth pairs for containers
/// and VMs. A tunnel's packets go out encrypted on en0 or eth0 as well.
const VIRTUAL: &[&str] = &[
    "lo",
    "utun",
    "tun",
    "tap",
    "wg",
    "ipsec",
    "gif",
    "stf",
    "tailscale",
    "zt",
    "docker",
    // Linux br0 and Docker's br-<id>, and macOS bridge100.
    "br",
    "veth",
    "virbr",
    "vmnet",
    "vboxnet",
    "vEthernet",
    "Loopback",
];

fn is_virtual(name: &str) -> bool {
    VIRTUAL.iter().any(|prefix| name.starts_with(prefix))
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
            .filter(|(name, _)| !is_virtual(name))
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
                        tone: None,
                        name: "down".into(),
                        points: vec![],
                    },
                    SeriesData {
                        tone: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_physical_interfaces_only() {
        for name in [
            "en0", "en1", "eth0", "wlan0", "wlp2s0", "enp3s0", "Ethernet", "Wi-Fi",
        ] {
            assert!(!is_virtual(name), "{name}");
        }
        for name in [
            "lo0",
            "utun17",
            "tun0",
            "wg0",
            "tailscale0",
            "docker0",
            "veth1a2b",
            "br-4f2e",
            "br0",
            "bridge100",
            "vEthernet (WSL)",
        ] {
            assert!(is_virtual(name), "{name}");
        }
    }
}
