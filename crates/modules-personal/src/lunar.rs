//! The moon's phase, computed here: no network, no data files.
//!
//! ```yaml
//! modules:
//!   lunar: {}
//! ```
//!
//! It uses the mean length of a lunar month from a known new moon, so the
//! phase and the next new and full moon can be off by up to about a day.

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{Action, CatalogEntry, ListItem, Manifest, Size, Status, View, Widget};
use dashboard_host::{Module, now_ms};
use dashboard_modules_common::manifest;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use schemars::JsonSchema;
use serde::Deserialize;

/// Mean days from one new moon to the next.
const SYNODIC_DAYS: f64 = 29.530_588_853;
/// A new moon: 2000-01-06 18:14 UTC, in unix seconds.
const NEW_MOON: f64 = 947_182_440.0;
const DAY: f64 = 86_400.0;
const PHASES_URL: &str = "https://www.timeanddate.com/moon/phases/";

/// No settings yet; the block exists so a typo is an error.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {}

#[derive(Default)]
pub struct Lunar;

#[async_trait]
impl Module for Lunar {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            let _: Config = serde_json::from_value(config)?;
        }
        Ok(manifest("lunar", "Moon", 3_600_000, 60_000, Size::Small, 0))
    }

    async fn render(&mut self) -> Result<View> {
        Ok(view(now_ms() / 1000, &TimeZone::system()))
    }
}

/// Where the moon is in its cycle at `secs` (unix seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Phase {
    /// Days since the last new moon.
    age: f64,
    /// Share of the disc that is lit, 0-100.
    illuminated: f64,
    /// Unix seconds of the next new and full moon.
    next_new: i64,
    next_full: i64,
}

fn phase(secs: i64) -> Phase {
    let since = (secs as f64 - NEW_MOON) / DAY;
    let cycles = (since / SYNODIC_DAYS).floor();
    let age = since - cycles * SYNODIC_DAYS;
    let fraction = age / SYNODIC_DAYS;
    let illuminated = (1.0 - (2.0 * std::f64::consts::PI * fraction).cos()) / 2.0 * 100.0;
    let at = |cycle: f64| (NEW_MOON + cycle * SYNODIC_DAYS * DAY).round() as i64;
    let next_new = at(cycles + 1.0);
    let full = at(cycles + 0.5);
    let next_full = if full > secs { full } else { at(cycles + 1.5) };
    Phase {
        age,
        illuminated,
        next_new,
        next_full,
    }
}

/// The phase's name: eight parts of the cycle, each centered on its point
/// (new, first quarter, full, last quarter) or between two of them.
fn name(age: f64) -> &'static str {
    const NAMES: [&str; 8] = [
        "New moon",
        "Waxing crescent",
        "First quarter",
        "Waxing gibbous",
        "Full moon",
        "Waning gibbous",
        "Last quarter",
        "Waning crescent",
    ];
    let eighth = (age / SYNODIC_DAYS * 8.0 + 0.5).floor() as usize % 8;
    NAMES[eighth]
}

/// "Thu Nov 5" in `tz`.
fn day(secs: i64, tz: &TimeZone) -> String {
    match Timestamp::from_second(secs) {
        Ok(t) => t.to_zoned(tz.clone()).strftime("%a %b %-d").to_string(),
        Err(_) => "-".into(),
    }
}

fn stat(label: &str, value: String, delta: Option<String>) -> Widget {
    Widget::Stat {
        label: label.into(),
        value,
        unit: None,
        delta,
        status: None,
    }
}

/// A line for the next new or full moon; it opens a table of exact times.
fn next(title: &str, secs: i64, tz: &TimeZone) -> ListItem {
    ListItem {
        title: title.into(),
        subtitle: Some(day(secs, tz)),
        status: None,
        tags: vec![],
        action: Some(Action::OpenUrl {
            url: PHASES_URL.into(),
        }),
    }
}

/// The tile at `secs`, with dates shown in `tz`.
fn view(secs: i64, tz: &TimeZone) -> View {
    let p = phase(secs);
    let name = name(p.age);
    let lit = p.illuminated.round();
    View {
        status: Status::Ok,
        summary: format!("{name} · {lit:.0}% lit"),
        root: Widget::Column {
            children: vec![
                stat(
                    "Moon",
                    name.into(),
                    Some(format!("day {:.0} of 29.5", p.age.floor())),
                ),
                Widget::Gauge {
                    label: "Illuminated".into(),
                    percent: lit,
                    marker: None,
                    thresholds: None,
                    caption: None,
                },
                Widget::List {
                    items: vec![
                        next("Next full moon", p.next_full, tz),
                        next("Next new moon", p.next_new, tz),
                    ],
                    empty_text: None,
                },
            ],
        },
    }
}

/// The demo's time: 2026-10-27 12:00 UTC.
const DEMO_SECS: i64 = 1_793_102_400;

pub fn catalog() -> CatalogEntry {
    CatalogEntry {
        description: "The moon's phase, how much of it is lit, and the next full and new \
                      moon. Computed here, with no network, from the mean lunar month, so \
                      it can be off by about a day."
            .into(),
        tags: crate::strings(&["moon", "astronomy", "personal", "fun"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry(
            "lunar",
            "Moon",
            Size::Small,
            view(DEMO_SECS, &TimeZone::UTC),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(ts: &str) -> i64 {
        ts.parse::<Timestamp>().unwrap().as_second()
    }

    #[test]
    fn known_phases() {
        // The total solar eclipse of 2024-04-08 was at a new moon.
        let p = phase(secs("2024-04-08T18:21:00Z"));
        assert_eq!(name(p.age), "New moon");
        assert!(p.illuminated < 2.0, "{p:?}");
        // Full moon 2024-04-23 23:49 UTC.
        let p = phase(secs("2024-04-23T23:49:00Z"));
        assert_eq!(name(p.age), "Full moon");
        assert!(p.illuminated > 98.0, "{p:?}");
        // First quarter 2024-04-15 19:13 UTC; last quarter 2024-05-01 11:27 UTC.
        assert_eq!(
            name(phase(secs("2024-04-15T19:13:00Z")).age),
            "First quarter"
        );
        assert_eq!(
            name(phase(secs("2024-05-01T11:27:00Z")).age),
            "Last quarter"
        );
        assert_eq!(
            name(phase(secs("2024-04-11T12:00:00Z")).age),
            "Waxing crescent"
        );
    }

    #[test]
    fn next_new_and_full_moon() {
        // Two days after the 2024-04-08 new moon: the full moon of 04-23 is
        // next (within a day of it), then the new moon of 05-08.
        let now = secs("2024-04-10T00:00:00Z");
        let p = phase(now);
        assert!((p.next_full - secs("2024-04-23T23:49:00Z")).abs() < DAY as i64);
        assert!((p.next_new - secs("2024-05-08T03:22:00Z")).abs() < DAY as i64);
        // After the full moon, the next one is a cycle later.
        let p = phase(secs("2024-04-25T00:00:00Z"));
        assert!(p.next_full > secs("2024-05-20T00:00:00Z"));
        assert!(p.next_new < p.next_full);
    }

    #[test]
    fn demo_view() {
        assert_eq!(DEMO_SECS, secs("2026-10-27T12:00:00Z"));
        let v = catalog().demo;
        assert_eq!(v.status, Status::Ok);
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::Gauge { percent, .. } = &children[1] else {
            panic!("gauge")
        };
        assert!((0.0..=100.0).contains(percent));
        assert!(v.summary.ends_with("% lit"), "{}", v.summary);
    }

    #[tokio::test]
    async fn config_errors() {
        assert!(Lunar.init(serde_json::json!({})).await.is_ok());
        assert!(Lunar.init(serde_json::json!({ "x": 1 })).await.is_err());
    }

    #[test]
    fn demo_is_valid() {
        let problems: Vec<String> = dashboard_contract::check::check_view(&catalog().demo)
            .into_iter()
            .filter(|f| f.problem)
            .map(|f| f.text)
            .collect();
        assert!(problems.is_empty(), "{problems:?}");
    }
}
