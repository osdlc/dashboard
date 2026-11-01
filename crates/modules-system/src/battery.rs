//! Battery: power source, charge and time remaining, as a gauge. Reads
//! `pmset -g batt` on macOS and `/sys/class/power_supply` elsewhere.
//! Only the machine's own battery is shown: on a Mac running from a UPS the
//! tile says "UPS power" and leaves out the UPS's own charge and time.
//!
//! ```yaml
//! modules:
//!   battery:
//!     config: { low: 20, critical: 10 }
//! ```
//!
//! This is one of the native-vs-plugin benchmark widgets: the TypeScript and
//! Python versions in `plugins/ext/battery-ts` and `plugins/ext/battery-py`
//! follow the same rules and must show the same view for the same input.
//! The shared samples and expected views are in `testdata/battery/`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

use dashboard_modules_common::{expand_home, manifest};

const SYSFS: &str = "/sys/class/power_supply";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// At or below this percent, on battery, the tile turns yellow.
    low: f64,
    /// At or below this percent, on battery, the tile turns red.
    critical: f64,
    /// Read this instead of the machine: a file with saved `pmset -g batt`
    /// output, or a folder laid out like `/sys/class/power_supply`. For tests
    /// and demos.
    sample: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            low: 20.0,
            critical: 10.0,
            sample: None,
        }
    }
}

/// What the power source reports.
#[derive(Debug, Clone, PartialEq)]
struct Reading {
    /// "AC power", "battery" or "UPS power".
    source: String,
    battery: Option<Charge>,
}

#[derive(Debug, Clone, PartialEq)]
struct Charge {
    /// Whole percent, 0-100.
    percent: f64,
    /// "charging", "discharging", "charged", "not charging" or what the
    /// system said, in lower case.
    state: String,
    /// To empty when discharging, to full when charging.
    minutes: Option<u64>,
}

#[derive(Default)]
pub struct Battery {
    config: Config,
}

#[async_trait]
impl Module for Battery {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest(
            "battery",
            "Battery",
            30_000,
            5_000,
            Size::Small,
            20,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let reading = match &self.config.sample {
            Some(path) => read_path(&expand_home(path))?,
            None if cfg!(target_os = "macos") => parse_pmset(&pmset().await?)?,
            None if cfg!(target_os = "linux") => read_sysfs(Path::new(SYSFS)),
            None => bail!("battery info is not available on {}", std::env::consts::OS),
        };
        Ok(view(&reading, self.config.low, self.config.critical))
    }
}

async fn pmset() -> Result<String> {
    let mut cmd = Command::new("pmset");
    cmd.args(["-g", "batt"])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(10), cmd.output())
        .await
        .context("pmset timed out")?
        .context("running pmset")?;
    if !out.status.success() {
        bail!(
            "pmset failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A saved sample: a folder is read as `/sys/class/power_supply`, a file as
/// `pmset -g batt` output.
fn read_path(path: &Path) -> Result<Reading> {
    if path.is_dir() {
        return Ok(read_sysfs(path));
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_pmset(&text)
}

/// Parses `pmset -g batt`:
///
/// ```text
/// Now drawing from 'Battery Power'
///  -InternalBattery-0 (id=4653155)    76%; discharging; 4:12 remaining present: true
/// ```
///
/// The gap before the percent is a tab. A Mac without a battery prints only
/// the first line.
fn parse_pmset(text: &str) -> Result<Reading> {
    let Some(from) = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Now drawing from '"))
    else {
        bail!("unexpected pmset output: {:?}", text.trim());
    };
    let from = from.trim_end_matches('\'');
    let source = if from.contains("Battery") {
        "battery"
    } else if from.contains("UPS") {
        "UPS power"
    } else {
        "AC power"
    };
    let battery = text
        .lines()
        .find(|l| l.trim_start().starts_with("-InternalBattery"))
        .and_then(|l| {
            let (_, fields) = l.split_once('\t')?;
            let parts: Vec<&str> = fields.split(';').map(str::trim).collect();
            let percent = parts
                .first()?
                .strip_suffix('%')?
                .parse::<f64>()
                .ok()
                .filter(|p| p.is_finite())?;
            let rest = parts.get(2).copied().unwrap_or("");
            let state = match parts.get(1).copied().unwrap_or("") {
                "finishing charge" => "charging".to_string(),
                "AC attached" => "not charging".to_string(),
                s => s.to_lowercase(),
            };
            Some(Charge {
                percent: percent.clamp(0.0, 100.0),
                minutes: remaining(rest).filter(|_| moving(&state)),
                state,
            })
        });
    Ok(Reading {
        source: source.into(),
        battery,
    })
}

/// "4:12 remaining present: true" -> 252.
fn remaining(text: &str) -> Option<u64> {
    let (time, _) = text.split_once(" remaining")?;
    let (h, m) = time.trim().split_once(':')?;
    // ASCII digits only, so "+4:12" is rejected as it is in the plugins.
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || !digits(m) {
        return None;
    }
    Some(h.parse::<u64>().ok()? * 60 + m.parse::<u64>().ok()?)
}

/// Only charging and discharging have a time worth showing; pmset says
/// "0:00 remaining" once charged.
fn moving(state: &str) -> bool {
    state == "charging" || state == "discharging"
}

/// Reads a `/sys/class/power_supply` folder: one folder per supply, each
/// with a `type` file. The first battery (by name) is shown. A battery whose
/// `scope` is `Device` belongs to a mouse, keyboard or other peripheral and is
/// skipped. No folder, or no battery in it, reads as AC power with no battery.
fn read_sysfs(dir: &Path) -> Reading {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    let mut ac = false;
    let mut battery = None;
    for supply in names {
        let file = |name: &str| {
            std::fs::read_to_string(supply.join(name))
                .ok()
                .map(|s| s.trim().to_string())
        };
        let number = |name: &str| {
            file(name)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite())
        };
        if file("type").as_deref() != Some("Battery") {
            ac |= file("online").as_deref() == Some("1");
            continue;
        }
        if battery.is_some()
            || file("present").as_deref() == Some("0")
            || file("scope").as_deref() == Some("Device")
        {
            continue;
        }
        let state = match file("status").as_deref() {
            Some("Full") => "charged".to_string(),
            Some(s) => s.to_lowercase(),
            None => "unknown".to_string(),
        };
        // Energy in µWh and µW, or charge in µAh and µA: either works for a ratio.
        let (now, full, rate) = match number("energy_now") {
            Some(now) => (Some(now), number("energy_full"), number("power_now")),
            None => (
                number("charge_now"),
                number("charge_full"),
                number("current_now"),
            ),
        };
        let percent = match (number("capacity"), now, full) {
            (Some(c), _, _) => c,
            (None, Some(now), Some(full)) if full > 0.0 => (now / full * 100.0 + 0.5).floor(),
            _ => 0.0,
        };
        // Some drivers report the rate as negative while discharging.
        let rate = rate.map(f64::abs);
        let minutes = match (state.as_str(), now, full, rate) {
            (_, _, _, Some(0.0)) => None,
            ("discharging", Some(now), _, Some(r)) => Some((now * 60.0 / r).floor() as u64),
            ("charging", Some(now), Some(full), Some(r)) => {
                Some(((full - now).max(0.0) * 60.0 / r).floor() as u64)
            }
            _ => None,
        };
        battery = Some(Charge {
            percent: percent.clamp(0.0, 100.0),
            state,
            minutes,
        });
    }
    let source = if !ac && battery.is_some() {
        "battery"
    } else {
        "AC power"
    };
    Reading {
        source: source.into(),
        battery,
    }
}

/// The tile. The plugin versions build exactly this.
fn view(r: &Reading, low: f64, critical: f64) -> View {
    let Some(b) = &r.battery else {
        return View {
            status: Status::Ok,
            summary: format!("{} · no battery", r.source),
            root: Widget::Text {
                text: format!("No battery found. Running on {}.", r.source),
                markdown: false,
            },
        };
    };
    let pct = b.percent as i64;
    let on_battery = b.state == "discharging";
    let status = if on_battery && b.percent <= critical {
        Status::Error
    } else if on_battery && b.percent <= low {
        Status::Warn
    } else {
        Status::Ok
    };
    let time = b
        .minutes
        .map(|m| format!("{}:{:02}", m / 60, m % 60))
        .unwrap_or_else(|| "-".into());
    let mut summary = format!("{pct}% · {}", b.state);
    match (b.state.as_str(), b.minutes) {
        ("discharging", Some(_)) => summary.push_str(&format!(" · {time} left")),
        ("charging", Some(_)) => summary.push_str(&format!(" · {time} to full")),
        _ => {}
    }
    let stat = |label: &str, value: String| Widget::Stat {
        label: label.into(),
        value,
        unit: None,
        delta: None,
        status: None,
    };
    View {
        status,
        summary,
        root: Widget::Column {
            children: vec![
                Widget::Gauge {
                    label: "Battery".into(),
                    percent: b.percent,
                    marker: None,
                    thresholds: None,
                    caption: Some(format!("on {}", r.source)),
                },
                Widget::Row {
                    children: vec![
                        stat("Power", r.source.clone()),
                        stat("State", b.state.clone()),
                        stat(
                            if b.state == "charging" {
                                "To full"
                            } else {
                                "Time left"
                            },
                            time,
                        ),
                    ],
                },
            ],
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let demo = view(
        &Reading {
            source: "battery".into(),
            battery: Some(Charge {
                percent: 76.0,
                state: "discharging".into(),
                minutes: Some(252),
            }),
        },
        20.0,
        10.0,
    );
    CatalogEntry {
        description: "Battery charge as a gauge, with the power source and the time left \
                      (or to full). Reads pmset on macOS and /sys/class/power_supply on Linux."
            .into(),
        tags: crate::strings(&["system", "local", "power"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: if cfg!(target_os = "macos") {
                crate::strings(&["pmset"])
            } else {
                vec![]
            },
            env: vec![],
            hosts: vec![],
        },
        ..crate::entry("battery", "Battery", Size::Small, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples shared with the plugin versions' tests in
    /// `crates/host/tests/bench_battery.rs`.
    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/battery")
            .join(name)
    }

    fn expected(name: &str) -> View {
        let text = std::fs::read_to_string(data(&format!("{name}.view.json"))).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn parses_pmset() {
        let r = parse_pmset(
            "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t76%; discharging; 4:12 remaining present: true\n",
        )
        .unwrap();
        assert_eq!(r.source, "battery");
        let b = r.battery.unwrap();
        assert_eq!(
            (b.percent, b.state.as_str(), b.minutes),
            (76.0, "discharging", Some(252))
        );

        let r = parse_pmset(
            "Now drawing from 'AC Power'\n -InternalBattery-0 (id=24641635)\t100%; charged; 0:00 remaining present: true\n",
        )
        .unwrap();
        assert_eq!(r.source, "AC power");
        assert_eq!(r.battery.unwrap().minutes, None, "no time once charged");

        let r = parse_pmset("Now drawing from 'AC Power'\n").unwrap();
        assert_eq!(r.battery, None);
        for odd in ["NaN", "inf", "0x10", ""] {
            let r = parse_pmset(&format!(
                "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t{odd}%; charging\n"
            ))
            .unwrap();
            assert_eq!(r.battery, None, "{odd}");
        }
        assert!(parse_pmset("pmset: command not found").is_err());
    }

    #[test]
    fn reads_sysfs() {
        let r = read_sysfs(&data("sysfs-charging"));
        assert_eq!(r.source, "AC power");
        let b = r.battery.unwrap();
        // charge_now 2.0 Ah of 5.0 Ah at 1.5 A: two hours to full.
        assert_eq!(
            (b.percent, b.state.as_str(), b.minutes),
            (40.0, "charging", Some(120))
        );
        assert_eq!(read_sysfs(Path::new("/no/such/folder")).battery, None);
    }

    #[test]
    fn samples_match_the_expected_views() {
        for name in SAMPLES {
            let file = match data(name) {
                dir if dir.is_dir() => dir,
                _ => data(&format!("{name}.txt")),
            };
            let got = view(&read_path(&file).unwrap(), 20.0, 10.0);
            // BLESS=1 rewrites the expected views after a deliberate change.
            if std::env::var_os("BLESS").is_some() {
                let json = serde_json::to_string_pretty(&got).unwrap() + "\n";
                std::fs::write(data(&format!("{name}.view.json")), json).unwrap();
            }
            assert_eq!(got, expected(name), "{name}");
        }
    }

    #[tokio::test]
    async fn sample_from_config() {
        let mut m = Battery::default();
        let path = data("pmset-low.txt");
        m.init(serde_json::json!({ "sample": path })).await.unwrap();
        let v = m.render().await.unwrap();
        assert_eq!(v.status, Status::Error);
        assert_eq!(v, expected("pmset-low"));
    }

    #[test]
    fn unknown_settings_are_rejected() {
        assert!(serde_json::from_value::<Config>(serde_json::json!({ "lwo": 5 })).is_err());
    }

    /// Every sample in testdata/battery: `<name>.txt` or the folder `<name>`,
    /// each with `<name>.view.json`.
    const SAMPLES: &[&str] = &[
        "pmset-ac",
        "pmset-charging",
        "pmset-desktop",
        "pmset-discharging",
        "pmset-finishing",
        "pmset-low",
        "pmset-ups",
        "sysfs-charging",
        "sysfs-desktop",
        "sysfs-discharging",
        "sysfs-negative-rate",
        "sysfs-peripheral-first",
    ];
}
