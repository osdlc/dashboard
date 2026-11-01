//! Current weather and a short forecast from Open-Meteo (no key needed), for a
//! place name or a latitude and longitude.
//!
//! ```yaml
//! modules:
//!   weather:
//!     config:
//!       place: Berlin       # looked up with Open-Meteo's geocoding API
//!       # or: { latitude: 52.52, longitude: 13.41 }
//!       name: Home          # shown on the tile; default the place's name
//!       units: metric       # or imperial
//!       days: 5             # forecast days, today first (1 to 16)
//! ```
//!
//! The same widget is also written as a TypeScript and a Python plugin
//! (`plugins/ext/weather-ts`, `plugins/ext/weather-py`) for the
//! native-vs-plugin benchmark. All three build the same view from the same
//! Open-Meteo reply, so a change to the view here goes into both plugins too.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, Cell, Column, Manifest, Requirements, Row, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{Client, client};
use dashboard_modules_common::{manifest, time::parse_time};
use schemars::JsonSchema;
use serde::Deserialize;

const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Units {
    /// °C, km/h.
    #[default]
    Metric,
    /// °F, mph.
    Imperial,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// A place name, looked up with Open-Meteo's geocoding API, e.g. "Berlin".
    /// Set this, or latitude and longitude.
    place: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    /// The name shown on the tile. Default: the place's name, or the coordinates.
    name: Option<String>,
    units: Units,
    /// Forecast days, today first (1 to 16).
    days: i64,
    /// The forecast API, e.g. a self-hosted Open-Meteo.
    forecast_url: String,
    /// The geocoding API, used for `place`.
    geocoding_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            place: None,
            latitude: None,
            longitude: None,
            name: None,
            units: Units::Metric,
            days: 5,
            forecast_url: FORECAST_URL.into(),
            geocoding_url: GEOCODING_URL.into(),
        }
    }
}

impl Config {
    /// Checks the types before serde reads them, so a wrong type gets the
    /// same message as from the plugins rather than serde's own.
    fn check_types(config: &serde_json::Value) -> Result<(), String> {
        let get = |key: &str| config.get(key).filter(|v| !v.is_null());
        for key in ["place", "name"] {
            if get(key).is_some_and(|v| !v.is_string()) {
                return Err(format!("{key} must be a string"));
            }
        }
        for key in ["latitude", "longitude"] {
            if get(key).is_some_and(|v| !v.is_number()) {
                return Err(format!("{key} must be a number"));
            }
        }
        for key in ["forecast_url", "geocoding_url"] {
            if config.get(key).is_some_and(|v| !v.is_string()) {
                return Err(format!("{key} must be a string"));
            }
        }
        if config
            .get("units")
            .is_some_and(|v| !matches!(v.as_str(), Some("metric" | "imperial")))
        {
            return Err("units must be metric or imperial".into());
        }
        if config.get("days").is_some_and(|v| v.as_i64().is_none()) {
            return Err("days must be a whole number".into());
        }
        Ok(())
    }

    /// What serde can't check. The plugins give the same messages.
    fn check(&self) -> Result<(), &'static str> {
        match (&self.place, self.latitude, self.longitude) {
            (Some(place), None, None) => {
                if place.trim().is_empty() {
                    return Err("place must not be empty");
                }
            }
            (None, Some(lat), Some(lon)) => {
                if !(-90.0..=90.0).contains(&lat) {
                    return Err("latitude must be between -90 and 90");
                }
                if !(-180.0..=180.0).contains(&lon) {
                    return Err("longitude must be between -180 and 180");
                }
            }
            (Some(_), _, _) => return Err("set place, or latitude and longitude, not both"),
            _ => return Err("set place, or latitude and longitude"),
        }
        if !(1..=16).contains(&self.days) {
            return Err("days must be between 1 and 16");
        }
        Ok(())
    }
}

/// Where the forecast is for, and the name shown for it.
#[derive(Debug, Clone)]
struct Location {
    latitude: f64,
    longitude: f64,
    label: String,
}

/// The parts of a forecast reply the tile uses.
#[derive(Debug, Deserialize)]
struct Forecast {
    current_units: CurrentUnits,
    current: Current,
    #[serde(default)]
    daily: Daily,
}

#[derive(Debug, Deserialize)]
struct CurrentUnits {
    temperature_2m: String,
    wind_speed_10m: String,
}

#[derive(Debug, Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    weather_code: u16,
    wind_speed_10m: f64,
}

/// One array per value, one entry per day. Any entry may be null.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Daily {
    time: Vec<String>,
    weather_code: Vec<Option<u16>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    precipitation_probability_max: Vec<Option<f64>>,
}

#[derive(Debug, Deserialize)]
struct Places {
    #[serde(default)]
    results: Vec<Place>,
}

#[derive(Debug, Deserialize)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Default)]
pub struct Weather {
    config: Config,
    /// Set from the coordinates at init, or by the first place lookup.
    location: Option<Location>,
}

#[async_trait]
impl Module for Weather {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            if config.is_object()
                && let Err(msg) = Config::check_types(&config)
            {
                bail!("{msg}");
            }
            self.config = serde_json::from_value(config)?;
        }
        if let Err(msg) = self.config.check() {
            bail!("{msg}");
        }
        let c = &self.config;
        self.location = match (c.latitude, c.longitude) {
            (Some(latitude), Some(longitude)) => Some(Location {
                latitude,
                longitude,
                label: c.name.clone().unwrap_or_else(|| {
                    format!("{}, {}", coordinate(latitude), coordinate(longitude))
                }),
            }),
            _ => None,
        };
        Ok(manifest(
            "weather",
            "Weather",
            900_000,
            60_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let http = client()?;
        let location = match &self.location {
            Some(l) => l.clone(),
            None => {
                let l = geocode(&http, &self.config).await?;
                self.location = Some(l.clone());
                l
            }
        };
        let c = &self.config;
        let mut query = vec![
            ("latitude", location.latitude.to_string()),
            ("longitude", location.longitude.to_string()),
            (
                "current",
                "temperature_2m,apparent_temperature,weather_code,wind_speed_10m".into(),
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max"
                    .into(),
            ),
            ("timezone", "auto".into()),
            ("forecast_days", c.days.to_string()),
        ];
        if c.units == Units::Imperial {
            query.push(("temperature_unit", "fahrenheit".into()));
            query.push(("wind_speed_unit", "mph".into()));
        }
        let reply = get(&http, &c.forecast_url, &query).await?;
        let forecast: Forecast =
            serde_json::from_value(reply).context("unexpected reply from Open-Meteo")?;
        Ok(view(&location.label, &forecast))
    }
}

/// Looks up `place` with the geocoding API.
async fn geocode(http: &Client, c: &Config) -> Result<Location> {
    let name = c.place.clone().unwrap_or_default();
    let query = [
        ("name", name.clone()),
        ("count", "1".into()),
        ("language", "en".into()),
        ("format", "json".into()),
    ];
    let reply = get(http, &c.geocoding_url, &query).await?;
    let places: Places =
        serde_json::from_value(reply).context("unexpected reply from Open-Meteo")?;
    let Some(place) = places.results.into_iter().next() else {
        bail!("no place named \"{name}\"");
    };
    Ok(Location {
        latitude: place.latitude,
        longitude: place.longitude,
        label: c.name.clone().unwrap_or(place.name),
    })
}

/// GETs `url` and returns the JSON reply. Open-Meteo explains a bad request
/// in a `reason` field.
async fn get(http: &Client, url: &str, query: &[(&str, String)]) -> Result<serde_json::Value> {
    let resp = http.get(url).query(query).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    let json: Option<serde_json::Value> = serde_json::from_str(&body).ok();
    if !status.is_success() {
        match json.as_ref().and_then(|j| j["reason"].as_str()) {
            Some(reason) => bail!("Open-Meteo: {reason}"),
            None => bail!("Open-Meteo: HTTP {}", status.as_u16()),
        }
    }
    json.context("unexpected reply from Open-Meteo")
}

/// What a WMO weather code means.
fn conditions(code: u16) -> String {
    let text = match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Rime fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Heavy drizzle",
        56 => "Light freezing drizzle",
        57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 => "Light freezing rain",
        67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Light showers",
        81 => "Showers",
        82 => "Heavy showers",
        85 => "Light snow showers",
        86 => "Snow showers",
        95 => "Thunderstorm",
        96 => "Thunderstorm, hail",
        99 => "Thunderstorm, heavy hail",
        n => return format!("Code {n}"),
    };
    text.into()
}

fn stormy(code: u16) -> bool {
    code >= 95
}

/// Rounded half away from zero, the same in all three versions (JavaScript's
/// Math.round and Python's round differ).
fn whole(x: f64) -> i64 {
    x.round() as i64
}

/// "52.52": two decimals, rounded the same way as `whole`.
fn coordinate(x: f64) -> String {
    let n = whole(x * 100.0);
    let sign = if n < 0 { "-" } else { "" };
    let n = n.abs();
    format!("{sign}{}.{:02}", n / 100, n % 100)
}

/// "Today" for the first day, then "Wed 28".
fn day_label(i: usize, date: &str) -> String {
    if i == 0 {
        return "Today".into();
    }
    const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let day: Option<u32> = date.get(8..10).and_then(|d| d.parse().ok());
    match (parse_time(&format!("{date}T00:00:00")), day) {
        // 1970-01-01 was a Thursday.
        (Some(secs), Some(day)) => {
            let weekday = (secs.div_euclid(86_400) + 4).rem_euclid(7) as usize;
            format!("{} {day}", NAMES[weekday])
        }
        _ => date.to_string(),
    }
}

fn stat(label: &str, value: f64, unit: &str, delta: Option<String>) -> Widget {
    Widget::Stat {
        label: label.into(),
        value: whole(value).to_string(),
        unit: Some(unit.into()),
        delta,
        status: None,
    }
}

fn column(key: &str, label: &str) -> Column {
    Column {
        key: key.into(),
        label: label.into(),
    }
}

/// The tile: now, then one row per day.
fn view(label: &str, f: &Forecast) -> View {
    let now = &f.current;
    let temp_unit = &f.current_units.temperature_2m;
    // Open-Meteo writes miles per hour as "mp/h".
    let wind_unit = match f.current_units.wind_speed_10m.as_str() {
        "mp/h" => "mph",
        u => u,
    };
    let sky = conditions(now.weather_code);
    let summary = format!("{label} · {}{temp_unit} · {sky}", whole(now.temperature_2m));

    let d = &f.daily;
    let degrees = |v: &Vec<Option<f64>>, i: usize| match v.get(i).copied().flatten() {
        Some(x) => format!("{}°", whole(x)),
        None => "-".into(),
    };
    let rows = d
        .time
        .iter()
        .enumerate()
        .map(|(i, date)| {
            let code = d.weather_code.get(i).copied().flatten();
            let rain = match d.precipitation_probability_max.get(i).copied().flatten() {
                Some(p) => format!("{}%", whole(p)),
                None => "-".into(),
            };
            let cells = [
                ("day", day_label(i, date)),
                ("sky", code.map_or("-".into(), conditions)),
                ("high", degrees(&d.temperature_2m_max, i)),
                ("low", degrees(&d.temperature_2m_min, i)),
                ("rain", rain),
            ];
            Row {
                cells: cells
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), Cell::Text(v)))
                    .collect(),
                status: code.filter(|c| stormy(*c)).map(|_| Status::Warn),
                action: None,
            }
        })
        .collect();

    View {
        status: if stormy(now.weather_code) {
            Status::Warn
        } else {
            Status::Ok
        },
        summary,
        root: Widget::Column {
            children: vec![
                Widget::Row {
                    children: vec![
                        stat(label, now.temperature_2m, temp_unit, Some(sky)),
                        stat("Feels like", now.apparent_temperature, temp_unit, None),
                        stat("Wind", now.wind_speed_10m, wind_unit, None),
                    ],
                },
                Widget::Table {
                    columns: vec![
                        column("day", "Day"),
                        column("sky", "Conditions"),
                        column("high", "High"),
                        column("low", "Low"),
                        column("rain", "Rain"),
                    ],
                    rows,
                },
            ],
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let forecast: Forecast =
        serde_json::from_str(include_str!("../testdata/weather/forecast.json"))
            .expect("demo data parses");
    CatalogEntry {
        description: "Current weather and a short forecast from Open-Meteo (no key needed), \
                      for a place name or a latitude and longitude."
            .into(),
        tags: crate::strings(&["weather", "personal", "web"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["api.open-meteo.com", "geocoding-api.open-meteo.com"]),
        },
        ..crate::entry(
            "weather",
            "Weather",
            Size::Medium,
            view("Berlin", &forecast),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = include_str!("../testdata/weather/forecast.json");
    const IMPERIAL: &str = include_str!("../testdata/weather/forecast-imperial.json");

    fn text(row: &Row, key: &str) -> String {
        match &row.cells[key] {
            Cell::Text(t) => t.clone(),
            other => panic!("{key}: {other:?}"),
        }
    }

    fn parts(v: &View) -> (&[Widget], &[Row]) {
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::Row { children: stats } = &children[0] else {
            panic!("row")
        };
        let Widget::Table { rows, .. } = &children[1] else {
            panic!("table")
        };
        (stats, rows)
    }

    #[test]
    fn parses_a_forecast() {
        let f: Forecast = serde_json::from_str(FORECAST).unwrap();
        let v = view("Berlin", &f);
        assert_eq!(v.status, Status::Ok);
        assert_eq!(v.summary, "Berlin · 10°C · Overcast");
        let (stats, rows) = parts(&v);
        let Widget::Stat {
            label,
            value,
            unit,
            delta,
            ..
        } = &stats[0]
        else {
            panic!("stat")
        };
        assert_eq!(
            (label.as_str(), value.as_str(), unit.as_deref()),
            ("Berlin", "10", Some("°C"))
        );
        assert_eq!(delta.as_deref(), Some("Overcast"));
        let Widget::Stat { value, .. } = &stats[1] else {
            panic!("stat")
        };
        assert_eq!(value, "7");

        assert_eq!(rows.len(), 5);
        // 2026-10-28 is a Wednesday.
        assert_eq!(text(&rows[0], "day"), "Today");
        assert_eq!(text(&rows[1], "day"), "Wed 28");
        assert_eq!(text(&rows[1], "sky"), "Light rain");
        assert_eq!(text(&rows[0], "high"), "14°");
        assert_eq!(text(&rows[0], "rain"), "15%");
        // Thunderstorm days are flagged.
        assert_eq!(rows[2].status, Some(Status::Warn));
        assert_eq!(rows[1].status, None);
        // Half away from zero, and no "-0".
        assert_eq!(text(&rows[2], "low"), "0°");
        assert_eq!(text(&rows[3], "high"), "3°");
        assert_eq!(text(&rows[3], "low"), "-3°");
        // Missing values.
        assert_eq!(text(&rows[4], "sky"), "-");
        assert_eq!(text(&rows[4], "high"), "-");
        assert_eq!(text(&rows[4], "rain"), "-");
    }

    #[test]
    fn imperial_units_and_storms() {
        let f: Forecast = serde_json::from_str(IMPERIAL).unwrap();
        let v = view("Home", &f);
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.summary, "Home · 50°F · Thunderstorm, hail");
        let (stats, rows) = parts(&v);
        let Widget::Stat { unit, .. } = &stats[2] else {
            panic!("stat")
        };
        assert_eq!(unit.as_deref(), Some("mph"));
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn helpers() {
        assert_eq!(conditions(42), "Code 42");
        assert_eq!(coordinate(52.52), "52.52");
        assert_eq!(coordinate(13.0), "13.00");
        assert_eq!(coordinate(13.125), "13.13");
        assert_eq!(coordinate(-0.004), "0.00");
        assert_eq!(coordinate(-33.8688), "-33.87");
        assert_eq!(day_label(3, "2026-10-31"), "Sat 31");
        assert_eq!(day_label(1, "2026-03-01"), "Sun 1");
        assert_eq!(day_label(1, "soon"), "soon");
    }

    #[tokio::test]
    async fn config_errors() {
        for (config, msg) in [
            (
                serde_json::json!({}),
                "set place, or latitude and longitude",
            ),
            (
                serde_json::json!({ "place": "Berlin", "latitude": 1.0 }),
                "set place, or latitude and longitude, not both",
            ),
            (
                serde_json::json!({ "latitude": 91.0, "longitude": 0.0 }),
                "latitude must be between -90 and 90",
            ),
            (
                serde_json::json!({ "place": "Berlin", "days": 0 }),
                "days must be between 1 and 16",
            ),
            (
                serde_json::json!({ "place": "Berlin", "days": -1 }),
                "days must be between 1 and 16",
            ),
            (
                serde_json::json!({ "place": "Berlin", "days": 2.5 }),
                "days must be a whole number",
            ),
            (
                serde_json::json!({ "place": "Berlin", "units": "kelvin" }),
                "units must be metric or imperial",
            ),
            (serde_json::json!({ "place": 3 }), "place must be a string"),
            (
                serde_json::json!({ "place": " " }),
                "place must not be empty",
            ),
        ] {
            let err = Weather::default().init(config).await.unwrap_err();
            assert_eq!(err.to_string(), msg);
        }
        let mut w = Weather::default();
        w.init(serde_json::json!({ "latitude": 52.52, "longitude": 13.0 }))
            .await
            .unwrap();
        assert_eq!(w.location.unwrap().label, "52.52, 13.00");
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
