//! Disks: read/write graph and space per volume.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, Manifest, SeriesData, Size, Status, ValueFormat, View, Widget, format_value,
};
use dashboard_host::{Module, now_ms};
use sysinfo::Disks;

use super::{
    Rate, demo_graph, demo_series, demo_values, gauge, graph, manifest, percent, sample,
    status_for, used_of,
};

const GIB: u64 = 1024 * 1024 * 1024;

pub struct Disk {
    disks: Disks,
    rate: Rate,
}

impl Default for Disk {
    fn default() -> Self {
        Self {
            disks: Disks::new_with_refreshed_list(),
            rate: Rate::default(),
        }
    }
}

/// Volumes worth showing: skips system-internal mounts (macOS has many under
/// /System/Volumes) and tiny ones, and shows each device once.
fn interesting(mount: &Path, total: u64) -> bool {
    let m = mount.to_string_lossy();
    total >= GIB
        && (m == "/"
            || !(m.starts_with("/System") || m.starts_with("/private") || m.starts_with("/dev")))
}

/// Bytes read and written, counting each device once: on macOS the APFS
/// volumes of one disk (`/` and `/System/Volumes/Data`) share a name and
/// report the same I/O counters, so a plain sum counts them twice. Two
/// different devices with the same name (two disk images mounted under one
/// name) are counted once, as the gauges show them; the total errs low.
fn io_sum<'a>(disks: impl Iterator<Item = (&'a std::ffi::OsStr, u64, u64)>) -> (u64, u64) {
    let mut seen = BTreeSet::new();
    disks
        .filter(|(name, ..)| seen.insert(*name))
        .fold((0, 0), |(r, w), (_, read, written)| (r + read, w + written))
}

#[async_trait]
impl Module for Disk {
    async fn init(&mut self, _config: serde_json::Value) -> Result<Manifest> {
        Ok(manifest("disk", "Disk", 2000, 500, Size::Medium, 70))
    }

    async fn render(&mut self) -> Result<View> {
        self.disks.refresh(true);
        let (read, written) = io_sum(self.disks.iter().map(|d| {
            let u = d.usage();
            (d.name(), u.read_bytes, u.written_bytes)
        }));

        let mut seen = BTreeSet::new();
        let mut gauges = vec![];
        let mut worst: f64 = 0.0;
        for d in self.disks.iter() {
            if !interesting(d.mount_point(), d.total_space()) || !seen.insert(d.name().to_owned()) {
                continue;
            }
            let used = d.total_space().saturating_sub(d.available_space());
            let pct = percent(used, d.total_space());
            worst = worst.max(pct);
            gauges.push(gauge(
                &d.mount_point().display().to_string(),
                pct,
                used_of(used, d.total_space()),
            ));
        }

        let now = now_ms();
        let (series, io) = match self.rate.elapsed() {
            Some(secs) => {
                let (r, w) = (read as f64 / secs, written as f64 / secs);
                let fmt = |v| format_value(v, ValueFormat::Bytes, Some("/s"));
                (
                    vec![sample("read", now, r), sample("write", now, w)],
                    format!(" · r {} w {}", fmt(r), fmt(w)),
                )
            }
            None => (
                vec![
                    SeriesData {
                        name: "read".into(),
                        points: vec![],
                    },
                    SeriesData {
                        name: "write".into(),
                        points: vec![],
                    },
                ],
                String::new(),
            ),
        };

        let mut children = vec![graph(
            "io",
            "Read / write",
            "/s",
            ValueFormat::Bytes,
            None,
            series,
        )];
        children.extend(gauges);
        Ok(View {
            status: status_for(worst),
            summary: format!("{worst:.0}% used{io}"),
            root: Widget::Column { children },
        })
    }
}

pub fn catalog() -> CatalogEntry {
    let read = demo_values(60, 900_000.0, 850_000.0, 1.5);
    let write = demo_values(60, 400_000.0, 380_000.0, 4.0);
    let demo = View {
        status: Status::Ok,
        summary: "71% used · r 1.2 MiB/s w 310 KiB/s".into(),
        root: Widget::Column {
            children: vec![
                demo_graph(
                    "Read / write",
                    "/s",
                    ValueFormat::Bytes,
                    None,
                    vec![
                        demo_series("read", 2000, &read),
                        demo_series("write", 2000, &write),
                    ],
                ),
                gauge("/", 71.0, used_of(355 * GIB, 500 * GIB)),
                gauge("/Volumes/Backup", 43.0, used_of(860 * GIB, 2000 * GIB)),
            ],
        },
    };
    CatalogEntry {
        description: "Disk reads and writes over time, and space used per volume.".into(),
        tags: crate::strings(&["system", "local", "storage"]),
        ..crate::entry("disk", "Disk", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn io_counts_each_device_once() {
        let hd = OsStr::new("Macintosh HD");
        let usb = OsStr::new("Backup");
        let disks = [(hd, 100, 10), (hd, 100, 10), (usb, 5, 1)];
        assert_eq!(io_sum(disks.into_iter()), (105, 11));
    }
}
