//! Clock: the local time in one or more time zones, as a row of stats. No
//! network: the zone rules come from the system's time zone database.
//!
//! ```yaml
//! modules:
//!   clock:
//!     config:
//!       zones: [local, UTC, America/New_York, { zone: Asia/Tokyo, label: Tokyo office }]
//!       hour12: false
//!       seconds: false
//! ```
//!
//! This is one of the native-vs-plugin benchmark widgets: the TypeScript and
//! Python versions in `plugins/ext/clock-ts` and `plugins/ext/clock-py`
//! follow the same rules and must show the same view for the same input.
//! The shared samples (configs with a fixed `now`) and expected views are in
//! `testdata/clock/`.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use schemars::JsonSchema;
use serde::Deserialize;

use dashboard_modules_common::manifest;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The time zones to show, in order: IANA names such as `Europe/Berlin`
    /// or `UTC`, or `local` for this machine's zone. An entry can also be
    /// `{ zone, label }` to name the stat.
    zones: Vec<ZoneEntry>,
    /// 12-hour time with AM and PM instead of 24-hour time.
    hour12: bool,
    /// Show seconds too (the tile then refreshes every second).
    seconds: bool,
    /// Show this time instead of the current one, written as
    /// `2026-10-27T14:05:00Z` or `2026-10-27T19:35:00+05:30` (years 1970 to
    /// 2999). For tests and demos.
    now: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            zones: vec![ZoneEntry::Name("local".into())],
            hour12: false,
            seconds: false,
            now: None,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum ZoneEntry {
    Name(String),
    Labeled(Labeled),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Labeled {
    zone: String,
    /// Shown above the time. By default the last part of the zone name.
    label: Option<String>,
}

/// One zone to show, looked up.
struct Zone {
    label: String,
    tz: TimeZone,
}

#[derive(Default)]
pub struct Clock {
    zones: Vec<Zone>,
    hour12: bool,
    seconds: bool,
    now: Option<Timestamp>,
}

#[async_trait]
impl Module for Clock {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        let config: Config = if config.is_null() {
            Config::default()
        } else {
            serde_json::from_value(config)?
        };
        if config.zones.is_empty() {
            bail!("zones is empty: list at least one time zone");
        }
        self.zones = config
            .zones
            .iter()
            .map(lookup)
            .collect::<Result<Vec<_>>>()?;
        self.now = match &config.now {
            Some(s) => Some(parse_now(s).with_context(|| {
                format!("now: expected a time like 2026-10-27T14:05:00Z, got {s:?}")
            })?),
            None => None,
        };
        self.hour12 = config.hour12;
        self.seconds = config.seconds;
        Ok(manifest(
            "clock",
            "Clock",
            if self.seconds { 1_000 } else { 5_000 },
            1_000,
            Size::Small,
            10,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let now = self.now.unwrap_or_else(Timestamp::now);
        Ok(view(&self.zones, now, self.hour12, self.seconds))
    }
}

fn lookup(entry: &ZoneEntry) -> Result<Zone> {
    let (name, label) = match entry {
        ZoneEntry::Name(name) => (name.as_str(), None),
        ZoneEntry::Labeled(l) => (l.zone.as_str(), l.label.clone()),
    };
    let tz = if name == "local" {
        TimeZone::system()
    } else if zone_name(name) {
        TimeZone::get(name).map_err(|_| anyhow::anyhow!("unknown time zone {name:?}"))?
    } else {
        bail!("unknown time zone {name:?}");
    };
    Ok(Zone {
        label: label.unwrap_or_else(|| default_label(name)),
        tz,
    })
}

/// Looks like an IANA zone name: a letter, then letters, digits and `_+-/`.
/// Node also takes offsets such as "+05:30" as zones; this keeps the three
/// versions in step.
fn zone_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_+-/".contains(c))
}

/// "America/New_York" -> "New York"; "local" -> "Local".
fn default_label(name: &str) -> String {
    if name == "local" {
        return "Local".into();
    }
    name.rsplit('/').next().unwrap_or(name).replace('_', " ")
}

/// Reads `YYYY-MM-DDTHH:MM:SS` followed by `Z` or `+HH:MM` / `-HH:MM`, and
/// nothing looser, so the three versions accept the same strings. The plugins
/// do the same checks and the same arithmetic.
fn parse_now(s: &str) -> Option<Timestamp> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = b.get(from..from + len)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    if !(at(4, b'-') && at(7, b'-') && at(10, b'T') && at(13, b':') && at(16, b':')) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let offset = match (b.len(), b.get(19)) {
        (20, Some(b'Z')) => 0,
        (25, Some(&sign @ (b'+' | b'-'))) if at(22, b':') => {
            let (h, m) = (num(20, 2)?, num(23, 2)?);
            if h > 23 || m > 59 {
                return None;
            }
            (h * 60 + m) * if sign == b'-' { -1 } else { 1 }
        }
        _ => return None,
    };
    if !(1970..=2999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second
        - offset * 60;
    Timestamp::from_second(secs).ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to the given date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The wall time in one zone: ("14:05", None) or ("2:05", Some("PM")), and
/// the date as "Tue 27 Oct".
fn wall(
    tz: &TimeZone,
    now: Timestamp,
    hour12: bool,
    seconds: bool,
) -> (String, Option<String>, String) {
    let z = now.to_zoned(tz.clone());
    let (h, m, s) = (z.hour(), z.minute(), z.second());
    let secs = if seconds {
        format!(":{s:02}")
    } else {
        String::new()
    };
    let (time, unit) = if hour12 {
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        let ampm = if h < 12 { "AM" } else { "PM" };
        (format!("{h12}:{m:02}{secs}"), Some(ampm.to_string()))
    } else {
        (format!("{h:02}:{m:02}{secs}"), None)
    };
    let weekday = WEEKDAYS[z.weekday().to_sunday_zero_offset() as usize];
    let date = format!("{weekday} {} {}", z.day(), MONTHS[z.month() as usize - 1]);
    (time, unit, date)
}

fn view(zones: &[Zone], now: Timestamp, hour12: bool, seconds: bool) -> View {
    let mut summary = vec![];
    let mut children = vec![];
    for zone in zones {
        let (time, unit, date) = wall(&zone.tz, now, hour12, seconds);
        summary.push(match &unit {
            Some(u) => format!("{} {time} {u}", zone.label),
            None => format!("{} {time}", zone.label),
        });
        children.push(Widget::Stat {
            label: zone.label.clone(),
            value: time,
            unit,
            delta: Some(date),
            status: None,
        });
    }
    View {
        status: Status::Ok,
        summary: summary.join(" · "),
        root: Widget::Row { children },
    }
}

pub fn catalog() -> CatalogEntry {
    let zones: Vec<Zone> = ["UTC", "America/New_York", "Asia/Tokyo"]
        .iter()
        .filter_map(|name| lookup(&ZoneEntry::Name(name.to_string())).ok())
        .collect();
    let now = parse_now("2026-10-27T14:05:00Z").expect("demo time");
    CatalogEntry {
        description: "The local time in one or more time zones, as a row of stats. \
                      Uses the system's time zone database; no network."
            .into(),
        tags: crate::strings(&["time", "local", "fun"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry(
            "clock",
            "Clock",
            Size::Small,
            view(&zones, now, false, false),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// Samples shared with the plugin versions' tests in
    /// `crates/host/tests/bench_clock.rs`: `<name>.json` is the tile's config
    /// (with a fixed `now`), `<name>.view.json` the view all three must show.
    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/clock")
            .join(name)
    }

    fn read(name: &str) -> serde_json::Value {
        let text = std::fs::read_to_string(data(name)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    async fn render(config: serde_json::Value) -> Result<View> {
        let mut m = Clock::default();
        m.init(config).await?;
        m.render().await
    }

    #[test]
    fn parses_now() {
        let at = |s: &str| parse_now(s).map(|t| t.as_second());
        assert_eq!(at("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(at("2026-10-27T14:05:09Z"), Some(1_793_109_909));
        assert_eq!(at("2026-10-27T19:35:09+05:30"), Some(1_793_109_909));
        assert_eq!(at("2026-10-27T10:05:09-04:00"), Some(1_793_109_909));
        assert_eq!(at("2028-02-29T00:00:00Z"), Some(1_835_395_200));
        for bad in [
            "",
            "2026-10-27",
            "2026-10-27T14:05Z",
            "2026-10-27 14:05:09Z",
            "2026-10-27t14:05:09z",
            "2026-10-27T14:05:09",
            "2026-10-27T14:05:09.5Z",
            "2026-10-27T14:05:09+0530",
            "2026-10-27T14:05:09+24:00",
            "2026-13-01T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2026-10-27T24:00:00Z",
            "2026-10-27T14:05:60Z",
            "1969-12-31T23:59:59Z",
            "+2026-10-27T14:05:09Z",
            "２026-10-27T14:05:09Z",
        ] {
            assert_eq!(at(bad), None, "{bad}");
        }
    }

    #[test]
    fn labels() {
        assert_eq!(default_label("America/New_York"), "New York");
        assert_eq!(
            default_label("America/Argentina/Buenos_Aires"),
            "Buenos Aires"
        );
        assert_eq!(default_label("UTC"), "UTC");
        assert_eq!(default_label("local"), "Local");
    }

    #[tokio::test]
    async fn samples_match_the_expected_views() {
        for name in SAMPLES {
            let got = render(read(&format!("{name}.json"))).await.unwrap();
            // BLESS=1 rewrites the expected views after a deliberate change.
            if std::env::var_os("BLESS").is_some() {
                let json = serde_json::to_string_pretty(&got).unwrap() + "\n";
                std::fs::write(data(&format!("{name}.view.json")), json).unwrap();
            }
            let expected: View =
                serde_json::from_value(read(&format!("{name}.view.json"))).unwrap();
            assert_eq!(got, expected, "{name}");
        }
    }

    #[tokio::test]
    async fn local_time_by_default() {
        let v = render(serde_json::Value::Null).await.unwrap();
        let Widget::Row { children } = &v.root else {
            panic!("{v:?}")
        };
        assert!(matches!(&children[..], [Widget::Stat { label, .. }] if label == "Local"));
        assert_eq!(v.status, Status::Ok);
    }

    #[tokio::test]
    async fn bad_settings_are_rejected() {
        for config in [
            serde_json::json!({ "zones": ["Mars/Olympus_Mons"] }),
            serde_json::json!({ "zones": ["+05:30"] }),
            serde_json::json!({ "zones": ["America"] }),
            serde_json::json!({ "zones": ["../zoneinfo/UTC"] }),
            serde_json::json!({ "zones": [] }),
            serde_json::json!({ "zones": [{ "zone": "UTC", "lable": "x" }] }),
            serde_json::json!({ "zones": "UTC" }),
            serde_json::json!({ "now": "yesterday" }),
            serde_json::json!({ "now": "2026-02-29T00:00:00Z" }),
            serde_json::json!({ "hour12": "yes" }),
            serde_json::json!({ "zone": ["UTC"] }),
        ] {
            assert!(render(config.clone()).await.is_err(), "{config}");
        }
    }

    #[test]
    fn catalog_demo_is_the_demo_sample() {
        // The catalog demo is the plugins' demo too (see their
        // dashboard-plugin.yaml).
        let expected: View = serde_json::from_value(read("demo.view.json")).unwrap();
        assert_eq!(catalog().demo, expected);
    }

    /// Every sample in testdata/clock.
    const SAMPLES: &[&str] = &["demo", "world", "hour12-seconds", "dst-start", "offset-now"];
}
