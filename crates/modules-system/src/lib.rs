//! The system bundle: CPU, memory, GPU, network, disk and processes (one module per
//! area, so each can be placed, sized and refreshed on its own), plus log
//! tailing, uptime checks, a text file viewer, the battery, a command's output
//! and the machine's security settings. Each module sits behind a cargo
//! feature named after its id.

use std::time::Instant;

use dashboard_contract::{
    ColorScale, SeriesData, SeriesStyle, Status, Thresholds, ValueFormat, Widget, format_value,
};
use dashboard_modules_common::Builtin;
pub use dashboard_modules_common::manifest;
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "battery")]
pub mod battery;
#[cfg(feature = "cmdrunner")]
pub mod cmdrunner;
#[cfg(feature = "cpu")]
pub mod cpu;
#[cfg(feature = "disk")]
pub mod disk;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "log")]
pub mod log;
#[cfg(feature = "memory")]
pub mod memory;
#[cfg(feature = "network")]
pub mod network;
#[cfg(feature = "processes")]
pub mod processes;
#[cfg(feature = "security")]
pub mod security;
#[cfg(feature = "textfile")]
pub mod textfile;
#[cfg(feature = "uptime")]
pub mod uptime;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "cpu")]
    Builtin {
        id: "cpu",
        core: true,
        new: || Box::new(cpu::Cpu::default()),
        catalog: cpu::catalog,
    },
    #[cfg(feature = "memory")]
    Builtin {
        id: "memory",
        core: true,
        new: || Box::new(memory::Memory::default()),
        catalog: memory::catalog,
    },
    #[cfg(feature = "gpu")]
    Builtin {
        id: "gpu",
        core: true,
        new: || Box::new(gpu::Gpu::default()),
        catalog: gpu::catalog,
    },
    #[cfg(feature = "network")]
    Builtin {
        id: "network",
        core: true,
        new: || Box::new(network::Network::default()),
        catalog: network::catalog,
    },
    #[cfg(feature = "disk")]
    Builtin {
        id: "disk",
        core: true,
        new: || Box::new(disk::Disk::default()),
        catalog: disk::catalog,
    },
    #[cfg(feature = "processes")]
    Builtin {
        id: "processes",
        core: true,
        new: || Box::new(processes::Processes::default()),
        catalog: processes::catalog,
    },
    #[cfg(feature = "log")]
    Builtin {
        id: "log",
        core: false,
        new: || Box::new(log::Log::default()),
        catalog: log::catalog,
    },
    #[cfg(feature = "uptime")]
    Builtin {
        id: "uptime",
        core: false,
        new: || Box::new(uptime::Uptime::default()),
        catalog: uptime::catalog,
    },
    #[cfg(feature = "textfile")]
    Builtin {
        id: "textfile",
        core: false,
        new: || Box::new(textfile::TextFile::default()),
        catalog: textfile::catalog,
    },
    #[cfg(feature = "battery")]
    Builtin {
        id: "battery",
        core: false,
        new: || Box::new(battery::Battery::default()),
        catalog: battery::catalog,
    },
    #[cfg(feature = "cmdrunner")]
    Builtin {
        id: "cmdrunner",
        core: false,
        new: || Box::new(cmdrunner::CmdRunner::default()),
        catalog: cmdrunner::catalog,
    },
    #[cfg(feature = "security")]
    Builtin {
        id: "security",
        core: false,
        new: || Box::new(security::Security),
        catalog: security::catalog,
    },
];

/// Points the host keeps per graph: two minutes at the default 500 ms.
pub const HISTORY: u32 = 240;
pub const THRESHOLDS: Thresholds = Thresholds {
    warn: 75.0,
    error: 90.0,
};

/// A graph whose history the host keeps; `points` are this update's samples.
pub fn graph(
    id: &str,
    label: &str,
    unit: &str,
    format: ValueFormat,
    y_max: Option<f64>,
    series: Vec<SeriesData>,
) -> Widget {
    Widget::Series {
        label: label.into(),
        unit: Some(unit.into()),
        y_max,
        id: Some(id.into()),
        history: Some(HISTORY),
        style: SeriesStyle::Area,
        color: if y_max.is_some() {
            ColorScale::Heat
        } else {
            ColorScale::Accent
        },
        mirrored: series.len() == 2,
        format,
        series,
    }
}

pub fn sample(name: &str, t: i64, v: f64) -> SeriesData {
    SeriesData {
        name: name.into(),
        points: vec![(t, v)],
    }
}

pub fn gauge(label: &str, percent: f64, caption: String) -> Widget {
    Widget::Gauge {
        label: label.into(),
        percent,
        marker: None,
        thresholds: Some(THRESHOLDS),
        caption: Some(caption),
    }
}

pub fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 / total as f64 * 100.0
    }
}

pub fn bytes(v: f64) -> String {
    format_value(v, ValueFormat::Bytes, None)
}

pub fn used_of(used: u64, total: u64) -> String {
    format!("{} / {}", bytes(used as f64), bytes(total as f64))
}

pub fn status_for(pct: f64) -> Status {
    if pct >= THRESHOLDS.error {
        Status::Error
    } else if pct >= THRESHOLDS.warn {
        Status::Warn
    } else {
        Status::Ok
    }
}

/// Turns "bytes since last refresh" counters into per-second rates.
#[derive(Default)]
pub struct Rate {
    last: Option<Instant>,
}

impl Rate {
    /// Seconds since the previous call, or `None` on the first call (no baseline yet).
    pub fn elapsed(&mut self) -> Option<f64> {
        let now = Instant::now();
        let secs = self
            .last
            .replace(now)
            .map(|t| now.duration_since(t).as_secs_f64());
        secs.filter(|s| *s > 0.0)
    }
}

// ---- demo data, for the catalog ----

/// `n` smooth made-up values around `base`, varying by up to `amp`. `seed`
/// shifts the shape so two series don't look alike.
pub fn demo_values(n: usize, base: f64, amp: f64, seed: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = i as f64 / 6.0 + seed;
            let v = (t * 0.9).sin() * 0.6 + (t * 2.3 + seed).sin() * 0.3 + (t * 5.1).sin() * 0.1;
            (base + amp * v).max(0.0)
        })
        .collect()
}

/// A demo series, one sample every `step_ms`, ending now.
pub fn demo_series(name: &str, step_ms: i64, values: &[f64]) -> SeriesData {
    let end = dashboard_host::now_ms();
    let n = values.len() as i64;
    SeriesData {
        name: name.into(),
        points: values
            .iter()
            .enumerate()
            .map(|(i, v)| (end - (n - 1 - i as i64) * step_ms, *v))
            .collect(),
    }
}

/// Like `graph`, with the full history in the view (no `id`), since no host
/// keeps a demo's history.
pub fn demo_graph(
    label: &str,
    unit: &str,
    format: ValueFormat,
    y_max: Option<f64>,
    series: Vec<SeriesData>,
) -> Widget {
    let mut w = graph("demo", label, unit, format, y_max, series);
    if let Widget::Series { id, history, .. } = &mut w {
        *id = None;
        *history = None;
    }
    w
}
