//! GPU: load over time, plus a utilization and a memory gauge per GPU.
//!
//! Where the numbers come from:
//! - macOS: `ioreg -c IOAccelerator`, its `PerformanceStatistics` dict. No sudo
//!   needed. Apple silicon GPUs share system memory, so the memory gauge is
//!   GPU memory in use out of all system memory.
//! - Linux: `nvidia-smi` for NVIDIA cards, and `/sys/class/drm/card*/device`
//!   (`gpu_busy_percent`, `mem_info_vram_*`) for AMD cards.
//! - Windows: `nvidia-smi` when it is installed.
//!
//! With none of these, the tile says there are no GPU stats.

use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, ValueFormat, View, Widget};
use dashboard_host::{Module, now_ms};
use sysinfo::System;
use tokio::process::Command;

use super::{
    demo_graph, demo_series, demo_values, gauge, graph, manifest, percent, sample, status_for,
    used_of,
};

/// One GPU's numbers from one reading.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuStat {
    pub name: String,
    /// Busy percent, 0–100.
    pub util: f64,
    /// GPU memory in use and the total it is out of, in bytes.
    pub mem: Option<(u64, u64)>,
    /// Extra facts for the detail line: cores, temperature, sub-unit loads.
    pub detail: String,
}

#[derive(Default)]
pub struct Gpu {
    sys: System,
}

#[async_trait]
impl Module for Gpu {
    async fn init(&mut self, _config: serde_json::Value) -> Result<Manifest> {
        Ok(manifest("gpu", "GPU", 1000, 500, Size::Medium, 95))
    }

    async fn render(&mut self) -> Result<View> {
        let mut gpus = read().await;
        if cfg!(target_os = "macos") && gpus.iter().any(|g| g.mem.is_some()) {
            // ioreg gives memory in use but no total: the GPU shares system memory.
            self.sys.refresh_memory();
            let total = self.sys.total_memory();
            for g in &mut gpus {
                if let Some((used, t)) = &mut g.mem
                    && *t == 0
                {
                    *t = total.max(*used);
                }
            }
        }
        Ok(view(&gpus, now_ms(), None))
    }
}

/// Reads every GPU this platform can report on. Empty when there is none.
async fn read() -> Vec<GpuStat> {
    let ioreg = ["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"];
    if cfg!(target_os = "macos")
        && let Some(out) = output("ioreg", &ioreg).await
    {
        let gpus = parse_ioreg(&out);
        if !gpus.is_empty() {
            return gpus;
        }
    }
    let mut gpus = vec![];
    if let Some(out) = output(
        "nvidia-smi",
        &[
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ],
    )
    .await
    {
        gpus.extend(parse_nvidia_smi(&out));
    }
    if cfg!(target_os = "linux") {
        gpus.extend(read_amd_sysfs());
    }
    gpus
}

/// A command's stdout, or `None` when it is missing, fails or hangs.
async fn output(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parses `ioreg -r -d 1 -w 0 -c IOAccelerator`: one `+-o` entry per GPU.
/// Memory comes back with a total of 0; the caller fills it in.
pub fn parse_ioreg(out: &str) -> Vec<GpuStat> {
    out.split("+-o ")
        .skip(1)
        .filter_map(|entry| {
            let stats = entry
                .lines()
                .find(|l| l.trim_start().starts_with("\"PerformanceStatistics\""))?;
            let num = |key: &str| dict_number(stats, key);
            let util = num("Device Utilization %")?;
            let name = entry
                .lines()
                .find_map(|l| string_value(l, "model"))
                .unwrap_or_else(|| "GPU".into());
            let mut detail = vec![];
            if let Some(cores) = entry
                .lines()
                .find_map(|l| number_value(l, "gpu-core-count"))
            {
                detail.push(format!("{cores:.0} cores"));
            }
            if let Some(r) = num("Renderer Utilization %") {
                detail.push(format!("renderer {r:.0}%"));
            }
            if let Some(t) = num("Tiler Utilization %") {
                detail.push(format!("tiler {t:.0}%"));
            }
            if let Some(a) = num("Alloc system memory") {
                detail.push(format!("{} allocated", super::bytes(a)));
            }
            Some(GpuStat {
                name,
                util: util.clamp(0.0, 100.0),
                mem: num("In use system memory").map(|u| (u as u64, 0)),
                detail: detail.join(" · "),
            })
        })
        .collect()
}

/// `"key"=123` inside an ioreg dict such as `{"a"=1,"key"=123}`.
fn dict_number(dict: &str, key: &str) -> Option<f64> {
    let pat = format!("\"{key}\"=");
    let rest = &dict[dict.find(&pat)? + pat.len()..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

/// The value of a top-level ioreg line `"key" = "value"`.
fn string_value(line: &str, key: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(&format!("\"{key}\" = "))?;
    Some(rest.trim_matches('"').to_string())
}

/// The value of a top-level ioreg line `"key" = 30`.
fn number_value(line: &str, key: &str) -> Option<f64> {
    line.trim()
        .strip_prefix(&format!("\"{key}\" = "))?
        .trim()
        .parse()
        .ok()
}

/// Parses `nvidia-smi --query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu
/// --format=csv,noheader,nounits`. Memory is in MiB. A field the card can't
/// report reads `[N/A]` (or `[Not Supported]`).
pub fn parse_nvidia_smi(out: &str) -> Vec<GpuStat> {
    const MIB: f64 = 1024.0 * 1024.0;
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 4 || f[0].is_empty() {
                return None;
            }
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok());
            let mem = match (num(2), num(3)) {
                (Some(u), Some(t)) if t > 0.0 => Some(((u * MIB) as u64, (t * MIB) as u64)),
                _ => None,
            };
            Some(GpuStat {
                name: f[0].to_string(),
                util: num(1).unwrap_or(0.0).clamp(0.0, 100.0),
                mem,
                detail: num(4).map(|t| format!("{t:.0}°C")).unwrap_or_default(),
            })
        })
        .collect()
}

/// AMD cards on Linux, from the amdgpu driver's sysfs files.
fn read_amd_sysfs() -> Vec<GpuStat> {
    let Ok(dir) = std::fs::read_dir("/sys/class/drm") else {
        return vec![];
    };
    let mut cards: Vec<_> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // card0, card1...; not the connectors (card0-HDMI-A-1).
            name.starts_with("card") && !name.contains('-')
        })
        .collect();
    cards.sort();
    cards
        .iter()
        .filter_map(|card| {
            let dev = card.join("device");
            let read = |f: &str| std::fs::read_to_string(dev.join(f)).ok();
            let name = card.file_name()?.to_string_lossy().into_owned();
            parse_amd(
                &name,
                &read("gpu_busy_percent")?,
                read("mem_info_vram_used").as_deref(),
                read("mem_info_vram_total").as_deref(),
            )
        })
        .collect()
}

/// One AMD card from the contents of its sysfs files.
pub fn parse_amd(
    card: &str,
    busy: &str,
    vram_used: Option<&str>,
    vram_total: Option<&str>,
) -> Option<GpuStat> {
    let util: f64 = busy.trim().parse().ok()?;
    let parse = |s: Option<&str>| s.and_then(|s| s.trim().parse::<u64>().ok());
    let mem = match (parse(vram_used), parse(vram_total)) {
        (Some(u), Some(t)) if t > 0 => Some((u, t)),
        _ => None,
    };
    Some(GpuStat {
        name: format!("AMD {card}"),
        util: util.clamp(0.0, 100.0),
        mem,
        detail: String::new(),
    })
}

/// The tile. `history` is the full demo history per GPU; `None` for a live
/// reading, whose samples the host appends to what it keeps.
fn view(gpus: &[GpuStat], t: i64, history: Option<Vec<Vec<f64>>>) -> View {
    if gpus.is_empty() {
        return View {
            status: Status::Unknown,
            summary: "no GPU stats".into(),
            root: Widget::Text {
                text: "No GPU stats on this machine. The tile reads ioreg on macOS, \
                       nvidia-smi for NVIDIA cards and amdgpu's sysfs files on Linux."
                    .into(),
                markdown: false,
            },
        };
    }
    let label = |i: usize, g: &GpuStat| {
        if gpus.len() == 1 {
            "load".to_string()
        } else {
            format!("{} {}", i + 1, g.name)
        }
    };
    let series = gpus
        .iter()
        .enumerate()
        .map(|(i, g)| match &history {
            Some(h) => demo_series(&label(i, g), 1000, &h[i]),
            None => sample(&label(i, g), t, g.util),
        })
        .collect();
    let mut chart = match history {
        Some(_) => demo_graph("Load", "%", ValueFormat::Plain, Some(100.0), series),
        None => graph("load", "Load", "%", ValueFormat::Plain, Some(100.0), series),
    };
    if let Widget::Series {
        style, mirrored, ..
    } = &mut chart
    {
        *style = dashboard_contract::SeriesStyle::Line;
        *mirrored = false;
    }

    let mut children = vec![chart];
    for g in gpus {
        let mut gauges = vec![gauge("Utilization", g.util, g.name.clone())];
        if let Some((used, total)) = g.mem {
            gauges.push(gauge("Memory", percent(used, total), used_of(used, total)));
        }
        children.push(Widget::Row { children: gauges });
        if !g.detail.is_empty() {
            children.push(Widget::Text {
                text: g.detail.clone(),
                markdown: false,
            });
        }
    }

    let worst = gpus.iter().map(|g| g.util).fold(0.0, f64::max);
    let summary = gpus
        .iter()
        .map(|g| format!("{:.0}%", g.util))
        .collect::<Vec<_>>()
        .join(" · ");
    View {
        status: status_for(worst),
        summary: format!("GPU {summary}"),
        root: Widget::Column { children },
    }
}

pub fn catalog() -> CatalogEntry {
    const GIB: u64 = 1024 * 1024 * 1024;
    let gpus = [GpuStat {
        name: "Apple M2 Max".into(),
        util: 23.0,
        mem: Some((3 * GIB, 32 * GIB)),
        detail: "30 cores · renderer 21% · tiler 18% · 13.8 GiB allocated".into(),
    }];
    let mut load = demo_values(120, 22.0, 18.0, 4.0);
    *load.last_mut().unwrap() = 23.0;
    CatalogEntry {
        description: "GPU load over time, with a utilization and a memory gauge per GPU. \
                      Reads ioreg on macOS, nvidia-smi for NVIDIA cards, and amdgpu's sysfs \
                      files on Linux."
            .into(),
        tags: crate::strings(&["system", "local", "gpu"]),
        ..crate::entry("gpu", "GPU", Size::Medium, view(&gpus, 0, Some(vec![load])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from an M2 Max.
    const IOREG: &str = r#"+-o AGXAcceleratorG14X  <class AGXAcceleratorG14X, id 0x100000433, registered, matched, active, busy 0 (108776 ms), retain 68>
    {
      "SchedulerState" = {"Stamps"=(),"BusyWorkQueues"=()}
      "IOMatchedAtBoot" = Yes
      "PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=14775713792,"Tiler Utilization %"=10,"recoveryCount"=0,"lastRecoveryTime"=0,"Renderer Utilization %"=12,"TiledSceneBytes"=1409024,"Device Utilization %"=14,"SplitSceneCount"=0,"Allocated PB Size"=105250816,"In use system memory"=752091136}
      "model" = "Apple M2 Max"
      "gpu-core-count" = 30
      "IOClass" = "AGXAcceleratorG14X"
    }
"#;

    #[test]
    fn parses_ioreg() {
        let gpus = parse_ioreg(IOREG);
        assert_eq!(gpus.len(), 1);
        let g = &gpus[0];
        assert_eq!(g.name, "Apple M2 Max");
        assert_eq!(g.util, 14.0);
        // "In use system memory", not "In use system memory (driver)".
        assert_eq!(g.mem, Some((752_091_136, 0)));
        assert!(g.detail.starts_with("30 cores · renderer 12% · tiler 10%"));
    }

    #[test]
    fn ioreg_without_stats_gives_nothing() {
        assert!(parse_ioreg("").is_empty());
        let no_stats = "+-o IOAccelerator  <class X>\n    {\n      \"model\" = \"X\"\n    }\n";
        assert!(parse_ioreg(no_stats).is_empty());
    }

    #[test]
    fn parses_nvidia_smi() {
        let out = "NVIDIA GeForce RTX 4090, 87, 18432, 24564, 71\n\
                   Tesla T4, 0, 0, 15360, [N/A]\n";
        let gpus = parse_nvidia_smi(out);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4090");
        assert_eq!(gpus[0].util, 87.0);
        assert_eq!(
            gpus[0].mem,
            Some((18432 * 1024 * 1024, 24564 * 1024 * 1024))
        );
        assert_eq!(gpus[0].detail, "71°C");
        assert_eq!(gpus[1].util, 0.0);
        assert_eq!(gpus[1].detail, "");
    }

    #[test]
    fn nvidia_smi_without_memory_has_no_memory_gauge() {
        let gpus = parse_nvidia_smi("Jetson, 40, [N/A], [N/A], 50\n");
        assert_eq!(gpus[0].mem, None);
        let v = view(&gpus, 1, None);
        let Widget::Column { children } = &v.root else {
            panic!("not a column")
        };
        let Widget::Row { children: gauges } = &children[1] else {
            panic!("not a row")
        };
        assert_eq!(gauges.len(), 1);
    }

    #[test]
    fn parses_amd_sysfs() {
        let g = parse_amd("card1", "55\n", Some("1073741824\n"), Some("8589934592\n")).unwrap();
        assert_eq!(g.util, 55.0);
        assert_eq!(g.mem, Some((1 << 30, 8 << 30)));
        assert!(parse_amd("card0", "", None, None).is_none());
    }

    #[test]
    fn view_has_a_line_per_gpu_and_a_gauge_row_each() {
        let gpus = parse_nvidia_smi("A, 95, 1, 2, 60\nB, 10, 1, 2, 40\n");
        let v = view(&gpus, 1, None);
        assert_eq!(v.status, Status::Error);
        assert_eq!(v.summary, "GPU 95% · 10%");
        let Widget::Column { children } = &v.root else {
            panic!("not a column")
        };
        let Widget::Series {
            series, mirrored, ..
        } = &children[0]
        else {
            panic!("not a series")
        };
        assert_eq!(series.len(), 2);
        assert!(!mirrored);
        let rows = children
            .iter()
            .filter(|c| matches!(c, Widget::Row { .. }))
            .count();
        assert_eq!(rows, 2);
    }

    #[test]
    fn no_gpu_says_so() {
        let v = view(&[], 1, None);
        assert_eq!(v.status, Status::Unknown);
        assert_eq!(v.summary, "no GPU stats");
    }
}
