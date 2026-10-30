//! Demo module: made-up, animated data in every widget style, for trying out
//! layouts and looks. Use it several times with `builtin: demo`, e.g.
//!
//! ```yaml
//! modules:
//!   demo-cool:
//!     builtin: demo
//!     config: { show: chart, style: bars, color: cool, signal: spiky }
//! ```

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::rpc::LogLevel;
use dashboard_contract::{
    Action, Bar, CatalogEntry, Cell, ColorScale, Column, ListItem, LogLine, Manifest, Meter, Row,
    SeriesData, SeriesStyle, Size, Status, Thresholds, ValueFormat, View, Widget,
};
use dashboard_host::{Module, now_ms};
use schemars::JsonSchema;
use serde::Deserialize;

use dashboard_modules_common::manifest;

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Show {
    /// One series in the configured style and color.
    #[default]
    Chart,
    /// Several lines on one chart.
    Lines,
    /// Up/down traffic around a middle axis.
    Mirrored,
    /// A grid of small meters.
    Meters,
    /// Gauges with "expected" markers.
    Gauges,
    /// A row of big numbers.
    Stats,
    /// Horizontal bars, sorted.
    Leaderboard,
    /// A table with gauges in cells.
    Table,
    /// A list with tags and statuses.
    Feed,
    /// Markdown text.
    Note,
    /// A bit of everything in one tile.
    Mixed,
    /// A scrolling made-up application log.
    Log,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum Signal {
    #[default]
    Wave,
    Noise,
    Spiky,
    Sawtooth,
    Steps,
    Pulse,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    show: Show,
    title: Option<String>,
    style: SeriesStyle,
    color: ColorScale,
    signal: Signal,
    /// Number of meters, bars, rows or items.
    count: usize,
    /// Changes the random numbers; tiles with the same settings otherwise look alike.
    seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show: Show::Chart,
            title: None,
            style: SeriesStyle::Area,
            color: ColorScale::Heat,
            signal: Signal::Wave,
            count: 0,
            seed: 1,
        }
    }
}

pub struct Demo {
    config: Config,
    start: Instant,
    rng: Rng,
    /// Random-walk state per series, for the noise and steps signals.
    walk: Vec<f64>,
    /// Lines for `show: log`, oldest first.
    log: std::collections::VecDeque<LogLine>,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            config: Config::default(),
            start: Instant::now(),
            rng: Rng(1),
            walk: vec![],
            log: Default::default(),
        }
    }
}

const NAMES: &[&str] = &[
    "atlas", "borealis", "cinder", "drift", "ember", "fjord", "glimmer", "harbor", "iris",
    "juniper", "kestrel", "lumen", "meridian", "nova", "orchid", "pylon", "quartz", "rook",
    "sable", "tundra",
];

#[async_trait]
impl Module for Demo {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        self.rng = Rng(self.config.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (size, interval) = match self.config.show {
            Show::Stats | Show::Note => (Size::Small, 1000),
            Show::Table | Show::Feed | Show::Leaderboard => (Size::Medium, 1000),
            Show::Mixed => (Size::Large, 250),
            Show::Log => (Size::Wide, 400),
            _ => (Size::Medium, 250),
        };
        let name = self.config.title.clone().unwrap_or_else(|| "Demo".into());
        Ok(manifest("demo", &name, interval, 100, size, 0))
    }

    async fn render(&mut self) -> Result<View> {
        let t = self.start.elapsed().as_secs_f64();
        let root = match self.config.show {
            Show::Chart => self.chart(t),
            Show::Lines => self.lines(t),
            Show::Mirrored => self.mirrored(t),
            Show::Meters => self.meters(t),
            Show::Gauges => self.gauges(t),
            Show::Stats => self.stats(t),
            Show::Leaderboard => self.leaderboard(t),
            Show::Table => self.table(t),
            Show::Feed => self.feed(t),
            Show::Note => note(),
            Show::Mixed => self.mixed(t),
            Show::Log => self.log(),
        };
        // Cycle the tile's status slowly so the status colors show up too.
        let status = match (t / 20.0) as u64 % 6 {
            4 => Status::Warn,
            5 => Status::Error,
            _ => Status::Ok,
        };
        let c = &self.config;
        let summary = match c.show {
            Show::Chart => format!("{:?} · {:?} · {:?}", c.style, c.color, c.signal),
            Show::Mirrored | Show::Meters => {
                format!("{:?} · {:?} · {:?}", c.show, c.color, c.signal)
            }
            Show::Mixed => format!("{:?} · {:?}", c.show, c.color),
            _ => format!("{:?}", c.show),
        }
        .to_lowercase();
        Ok(View {
            status,
            summary,
            root,
        })
    }
}

impl Demo {
    fn count(&self, default: usize) -> usize {
        if self.config.count == 0 {
            default
        } else {
            self.config.count
        }
    }

    /// Value of signal `sig` for series `i` at time `t`, in 0..100.
    fn signal(&mut self, sig: Signal, i: usize, t: f64) -> f64 {
        if self.walk.len() <= i {
            self.walk.resize(i + 1, 50.0);
        }
        let phase = i as f64 * 1.7 + self.config.seed as f64;
        let v = match sig {
            Signal::Wave => {
                50.0 + 32.0 * (t * 0.7 + phase).sin() + 12.0 * (t * 2.9 + phase * 2.0).sin()
            }
            Signal::Noise => {
                self.walk[i] = (self.walk[i] + self.rng.range(-9.0, 9.0)).clamp(3.0, 97.0);
                self.walk[i]
            }
            Signal::Spiky => {
                if self.rng.next_f64() < 0.06 {
                    self.rng.range(70.0, 100.0)
                } else {
                    self.rng.range(5.0, 22.0)
                }
            }
            Signal::Sawtooth => (t * 18.0 + phase * 20.0) % 100.0,
            Signal::Steps => {
                if self.rng.next_f64() < 0.05 {
                    self.walk[i] = self.rng.range(10.0, 95.0);
                }
                self.walk[i]
            }
            Signal::Pulse => {
                let beat = (t + phase) % 1.2;
                8.0 + 85.0 * (-beat * 9.0).exp() + 25.0 * (-((beat - 0.35) * 14.0).powi(2)).exp()
            }
        };
        v.clamp(0.0, 100.0)
    }

    fn series(
        &self,
        id: &str,
        label: &str,
        unit: &str,
        y_max: Option<f64>,
        format: ValueFormat,
        series: Vec<SeriesData>,
    ) -> Widget {
        let mirrored = id == "mirrored";
        Widget::Series {
            label: label.into(),
            unit: Some(unit.into()),
            y_max,
            id: Some(id.into()),
            history: Some(240),
            style: if mirrored {
                SeriesStyle::Area
            } else {
                self.config.style
            },
            color: self.config.color,
            mirrored,
            format,
            series,
        }
    }

    fn chart(&mut self, t: f64) -> Widget {
        let v = self.signal(self.config.signal, 0, t);
        let label = format!("{:?}", self.config.signal).to_lowercase();
        self.series(
            "chart",
            &label,
            "%",
            Some(100.0),
            ValueFormat::Plain,
            vec![point("value", v)],
        )
    }

    fn lines(&mut self, t: f64) -> Widget {
        let names = ["p50", "p90", "p99", "max"];
        let series = (0..self.count(3).min(4))
            .map(|i| {
                let v = self.signal(Signal::Wave, i, t) * (0.4 + i as f64 * 0.2);
                point(names[i], v.min(100.0))
            })
            .collect();
        let mut w = self.series(
            "lines",
            "Latency (ms)",
            " ms",
            Some(100.0),
            ValueFormat::Plain,
            series,
        );
        if let Widget::Series { style, .. } = &mut w {
            *style = SeriesStyle::Line;
        }
        w
    }

    fn mirrored(&mut self, t: f64) -> Widget {
        let down = self.signal(self.config.signal, 0, t) * 40_000.0;
        let up = self.signal(Signal::Spiky, 1, t) * 12_000.0;
        self.series(
            "mirrored",
            "Traffic",
            "/s",
            None,
            ValueFormat::Bytes,
            vec![point("down", down), point("up", up)],
        )
    }

    fn meters(&mut self, t: f64) -> Widget {
        let meters = (0..self.count(24))
            .map(|i| Meter {
                label: format!("{}", i + 1),
                percent: self.signal(self.config.signal, i, t + i as f64 * 0.137),
            })
            .collect();
        Widget::MeterGrid {
            label: self.config.title.clone(),
            meters,
            color: self.config.color,
        }
    }

    fn gauges(&mut self, t: f64) -> Widget {
        let labels = [
            "Build cache",
            "Token budget",
            "Disk quota",
            "Error budget",
            "Sprint",
        ];
        let children = labels
            .iter()
            .take(self.count(4))
            .enumerate()
            .map(|(i, label)| {
                let pct = self.signal(Signal::Wave, i, t * 0.3);
                Widget::Gauge {
                    label: (*label).into(),
                    percent: pct,
                    marker: Some((t * 2.0 + i as f64 * 17.0) % 100.0),
                    thresholds: Some(Thresholds {
                        warn: 70.0,
                        error: 90.0,
                    }),
                    caption: Some(format!("{:.0} of 100 · marker = where you should be", pct)),
                }
            })
            .collect();
        Widget::Column { children }
    }

    fn stats(&mut self, t: f64) -> Widget {
        let rps = 1200.0 + self.signal(Signal::Noise, 0, t) * 30.0;
        let err = self.signal(Signal::Spiky, 1, t) / 20.0;
        let p99 = 80.0 + self.signal(Signal::Wave, 2, t);
        Widget::Row {
            children: vec![
                stat(
                    "Requests",
                    format!("{rps:.0}"),
                    "/s",
                    format!("{:+.1}%", (t * 0.5).sin() * 4.0),
                    Status::Ok,
                ),
                stat(
                    "Errors",
                    format!("{err:.2}"),
                    "%",
                    "last 1m".into(),
                    if err > 2.0 { Status::Error } else { Status::Ok },
                ),
                stat(
                    "p99",
                    format!("{p99:.0}"),
                    "ms",
                    "target 150".into(),
                    if p99 > 150.0 {
                        Status::Warn
                    } else {
                        Status::Ok
                    },
                ),
            ],
        }
    }

    fn leaderboard(&mut self, t: f64) -> Widget {
        let mut bars: Vec<Bar> = NAMES
            .iter()
            .take(self.count(8))
            .enumerate()
            .map(|(i, name)| {
                let value = self.signal(Signal::Wave, i, t * 0.2) * 12.0;
                let status = match value {
                    v if v > 1000.0 => Some(Status::Error),
                    v if v > 800.0 => Some(Status::Warn),
                    _ => None,
                };
                Bar {
                    label: (*name).into(),
                    value,
                    status,
                }
            })
            .collect();
        bars.sort_by(|a, b| b.value.total_cmp(&a.value));
        Widget::Bars {
            label: Some("Builds this week".into()),
            unit: None,
            format: ValueFormat::Plain,
            max: Some(1200.0),
            bars,
        }
    }

    fn table(&mut self, t: f64) -> Widget {
        let columns = [
            ("svc", "Service"),
            ("region", "Region"),
            ("load", "Load"),
            ("ver", "Version"),
        ]
        .map(|(key, label)| Column {
            key: key.into(),
            label: label.into(),
        })
        .to_vec();
        let regions = ["us-east", "eu-west", "ap-south"];
        let rows = NAMES
            .iter()
            .take(self.count(6))
            .enumerate()
            .map(|(i, name)| {
                let load = self.signal(Signal::Noise, i, t);
                let status = match load {
                    l if l > 90.0 => Some(Status::Error),
                    l if l > 75.0 => Some(Status::Warn),
                    _ => Some(Status::Ok),
                };
                let gauge = Widget::Gauge {
                    label: String::new(),
                    percent: load,
                    marker: None,
                    thresholds: Some(Thresholds {
                        warn: 75.0,
                        error: 90.0,
                    }),
                    caption: None,
                };
                let cells = BTreeMap::from([
                    ("svc".to_string(), Cell::Text((*name).into())),
                    ("region".to_string(), Cell::Text(regions[i % 3].into())),
                    ("load".to_string(), Cell::Widget(gauge)),
                    (
                        "ver".to_string(),
                        Cell::Text(format!("v1.{}.{}", 4 + i % 3, i * 7 % 10)),
                    ),
                ]);
                Row {
                    cells,
                    status,
                    action: None,
                }
            })
            .collect();
        Widget::Table { columns, rows }
    }

    fn feed(&mut self, t: f64) -> Widget {
        let events = [
            ("Deploy finished", "ok", Status::Ok, &["prod"][..]),
            (
                "Flaky test quarantined",
                "ci",
                Status::Warn,
                &["tests", "auto"][..],
            ),
            ("Build failed on main", "ci", Status::Error, &["main"][..]),
            ("PR merged", "git", Status::Ok, &[][..]),
            ("Cache warmed", "infra", Status::Unknown, &["cache"][..]),
            (
                "Rollback started",
                "deploy",
                Status::Warn,
                &["prod", "manual"][..],
            ),
        ];
        // Shift by one every few seconds so the list visibly moves.
        let shift = (t / 4.0) as usize;
        let items = (0..self.count(5))
            .map(|i| {
                let (title, src, status, tags) = events[(i + shift) % events.len()];
                ListItem {
                    title: format!("{title} · {}", NAMES[(i + shift * 3) % NAMES.len()]),
                    subtitle: Some(format!("{src} · {}s ago", (i * 37 + shift) % 300)),
                    status: Some(status),
                    tags: tags.iter().map(|s| s.to_string()).collect(),
                    action: Some(Action::OpenUrl {
                        url: "https://example.com".into(),
                    }),
                }
            })
            .collect();
        Widget::List {
            items,
            empty_text: None,
        }
    }

    /// Adds zero to three lines per refresh, so the log moves at an uneven pace.
    fn log(&mut self) -> Widget {
        const MESSAGES: &[(LogLevel, &str)] = &[
            (LogLevel::Info, "GET /api/builds 200 in {}ms"),
            (LogLevel::Info, "worker {} picked up job"),
            (LogLevel::Debug, "cache hit for key build:{}"),
            (LogLevel::Info, "deployed revision r{} to staging"),
            (LogLevel::Warn, "slow query took {}ms"),
            (LogLevel::Warn, "retrying upload (attempt {})"),
            (LogLevel::Error, "connection reset by peer after {} bytes"),
            (LogLevel::Debug, "gc pause {}ms"),
            (LogLevel::Info, "POST /api/runs 201 in {}ms"),
        ];
        let max = self.count(300);
        for _ in 0..(self.rng.next_f64() * 3.5) as usize {
            let i = (self.rng.next_f64() * MESSAGES.len() as f64) as usize % MESSAGES.len();
            let (level, msg) = MESSAGES[i];
            let n = (self.rng.next_f64() * 900.0) as u32 + 1;
            let secs = now_ms() / 1000;
            let time = format!(
                "{:02}:{:02}:{:02}",
                secs / 3600 % 24,
                secs / 60 % 60,
                secs % 60
            );
            self.log.push_back(LogLine {
                text: msg.replace("{}", &n.to_string()),
                level: Some(level),
                time: Some(time),
            });
            while self.log.len() > max {
                self.log.pop_front();
            }
        }
        Widget::Log {
            label: self.config.title.clone(),
            lines: self.log.iter().cloned().collect(),
        }
    }

    fn mixed(&mut self, t: f64) -> Widget {
        let chart = self.chart(t);
        let meters = {
            let meters = (0..8)
                .map(|i| Meter {
                    label: format!("{}", i + 1),
                    percent: self.signal(Signal::Noise, i + 10, t),
                })
                .collect();
            Widget::MeterGrid {
                label: None,
                meters,
                color: self.config.color,
            }
        };
        Widget::Column {
            children: vec![
                self.stats(t),
                chart,
                meters,
                Widget::Gauge {
                    label: "Progress".into(),
                    percent: (t * 3.0) % 100.0,
                    marker: Some(60.0),
                    thresholds: None,
                    caption: Some("loops every ~33s".into()),
                },
            ],
        }
    }
}

pub fn catalog() -> CatalogEntry {
    let mut d = Demo::default();
    let t = 42.0;
    let demo = View {
        status: Status::Ok,
        summary: "made-up data in every widget style".into(),
        root: Widget::Column {
            children: vec![d.stats(t), d.gauges(t), d.leaderboard(t)],
        },
    };
    CatalogEntry {
        description: "Made-up, moving data in every widget style, for trying layouts and \
                      themes. Add it several times with `builtin: demo` and different settings."
            .into(),
        tags: crate::strings(&["demo", "local"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("demo", "Demo", Size::Medium, demo)
    }
}

fn note() -> Widget {
    Widget::Text {
        text: "**Demo tile.** Every tile on these screens is the `demo` module with different \
               settings: `show`, `style` (line, area, bars), `color` (accent, heat, cool, sunset) \
               and `signal` (wave, noise, spiky, sawtooth, steps, pulse). Edit the config while \
               it runs to try combinations."
            .into(),
        markdown: true,
    }
}

fn point(name: &str, v: f64) -> SeriesData {
    SeriesData {
        name: name.into(),
        points: vec![(now_ms(), v)],
    }
}

fn stat(label: &str, value: String, unit: &str, delta: String, status: Status) -> Widget {
    Widget::Stat {
        label: label.into(),
        value,
        unit: Some(unit.into()),
        delta: Some(delta),
        status: Some(status),
    }
}

/// Small xorshift generator; the demo doesn't need a real one.
struct Rng(u64);

impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
}
