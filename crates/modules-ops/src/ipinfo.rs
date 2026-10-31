//! This machine's public IP address and where it appears to be, from
//! ipinfo.io (no key needed for this lookup).
//!
//! ```yaml
//! modules:
//!   ipinfo:
//!     refresh: 10m
//!     config:
//!       url: https://ipinfo.io/json   # any server that answers in ipinfo's format
//! ```
//!
//! When the address changes between refreshes, the tile says what it was.

use anyhow::{Context, Result};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;

const URL: &str = "https://ipinfo.io/json";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The lookup API. It must answer like ipinfo.io's `/json`.
    url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { url: URL.into() }
    }
}

/// ipinfo.io's reply. Every field but `ip` may be missing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Info {
    ip: String,
    hostname: Option<String>,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
    /// "52.5244,13.4105"
    loc: Option<String>,
    /// "AS3320 Deutsche Telekom AG"
    org: Option<String>,
    timezone: Option<String>,
}

#[derive(Default)]
pub struct IpInfo {
    config: Config,
    /// The address from the last refresh, to notice a change.
    last_ip: Option<String>,
}

#[async_trait]
impl Module for IpInfo {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest(
            "ipinfo",
            "Public IP",
            600_000,
            60_000,
            Size::Small,
            0,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let resp = client()?.get(&self.config.url).send().await?;
        let info: Info = check(resp)
            .await?
            .json()
            .await
            .context("unexpected reply from the IP lookup")?;
        if info.ip.is_empty() {
            anyhow::bail!("the IP lookup gave no address");
        }
        // The old address, only on the refresh that sees the change: the
        // note shows once, until the next refresh.
        let before = match &self.last_ip {
            Some(ip) if *ip != info.ip => Some(ip.clone()),
            _ => None,
        };
        self.last_ip = Some(info.ip.clone());
        Ok(view(&info, before.as_deref()))
    }
}

/// "Berlin, Berlin, DE", leaving out parts that are missing or repeat.
fn place(info: &Info) -> Option<String> {
    let mut parts: Vec<&str> = vec![];
    for p in [&info.city, &info.region, &info.country]
        .into_iter()
        .flatten()
    {
        let p = p.trim();
        if !p.is_empty() && !parts.contains(&p) {
            parts.push(p);
        }
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// The tile: the address, then a line each for place, network and time zone.
/// `before` is the previous address when it just changed.
fn view(info: &Info, before: Option<&str>) -> View {
    let place = place(info);
    let link = |url: String| Some(Action::OpenUrl { url });
    let mut items = vec![];
    if let Some(p) = &place {
        items.push(ListItem {
            title: p.clone(),
            subtitle: info.loc.clone(),
            status: None,
            tags: vec!["location".into()],
            action: info
                .loc
                .as_ref()
                .and_then(|loc| {
                    let (lat, lon) = loc.split_once(',')?;
                    Some(format!(
                        "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=10/{lat}/{lon}"
                    ))
                })
                .and_then(link),
        });
    }
    if let Some(org) = info.org.as_ref().filter(|o| !o.is_empty()) {
        // "AS3320 Deutsche Telekom AG": the AS number links to its page.
        let asn = org
            .split_whitespace()
            .next()
            .filter(|a| a.starts_with("AS"));
        items.push(ListItem {
            title: org.clone(),
            subtitle: info.hostname.clone(),
            status: None,
            tags: vec!["network".into()],
            action: asn.and_then(|a| link(format!("https://ipinfo.io/{a}"))),
        });
    }
    if let Some(tz) = info.timezone.as_ref().filter(|t| !t.is_empty()) {
        items.push(ListItem {
            title: tz.clone(),
            subtitle: None,
            status: None,
            tags: vec!["time zone".into()],
            action: None,
        });
    }
    let summary = match (&place, before) {
        (_, Some(old)) => format!("{} · changed from {old}", info.ip),
        (Some(p), None) => format!("{} · {p}", info.ip),
        (None, None) => info.ip.clone(),
    };
    View {
        status: Status::Ok,
        summary,
        root: Widget::Column {
            children: vec![
                Widget::Stat {
                    label: "Public IP".into(),
                    value: info.ip.clone(),
                    unit: None,
                    delta: before.map(|old| format!("was {old}")),
                    status: before.map(|_| Status::Warn),
                },
                Widget::List {
                    items,
                    empty_text: Some("No location for this address.".into()),
                },
            ],
        },
    }
}

const SAMPLE: &str = include_str!("../testdata/ipinfo/reply.json");

pub fn catalog() -> CatalogEntry {
    let info: Info = serde_json::from_str(SAMPLE).expect("demo data parses");
    CatalogEntry {
        description: "This machine's public IP address, with its city, network and time \
                      zone, from ipinfo.io (no key needed). Notes when the address changes."
            .into(),
        tags: crate::strings(&["network", "ip", "ops", "web"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["ipinfo.io"]),
        },
        ..crate::entry("ipinfo", "Public IP", Size::Small, view(&info, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(v: &View) -> (&Widget, &[ListItem]) {
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::List { items, .. } = &children[1] else {
            panic!("list")
        };
        (&children[0], items)
    }

    #[test]
    fn parses_the_sample() {
        let info: Info = serde_json::from_str(SAMPLE).unwrap();
        let v = view(&info, None);
        assert_eq!(v.summary, "203.0.113.42 · Berlin, DE");
        let (stat, items) = parts(&v);
        let Widget::Stat { value, delta, .. } = stat else {
            panic!("stat")
        };
        assert_eq!(value, "203.0.113.42");
        assert_eq!(delta, &None);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "Berlin, DE");
        assert_eq!(
            items[0].action,
            Some(Action::OpenUrl {
                url: "https://www.openstreetmap.org/?mlat=52.5244&mlon=13.4105#map=10/52.5244/13.4105"
                    .into()
            })
        );
        assert_eq!(
            items[1].action,
            Some(Action::OpenUrl {
                url: "https://ipinfo.io/AS3320".into()
            })
        );
        assert_eq!(items[2].title, "Europe/Berlin");
    }

    #[test]
    fn a_changed_address_and_a_bare_reply() {
        let info: Info = serde_json::from_str(SAMPLE).unwrap();
        let v = view(&info, Some("198.51.100.7"));
        assert_eq!(v.summary, "203.0.113.42 · changed from 198.51.100.7");
        let (stat, _) = parts(&v);
        let Widget::Stat { delta, status, .. } = stat else {
            panic!("stat")
        };
        assert_eq!(delta.as_deref(), Some("was 198.51.100.7"));
        assert_eq!(*status, Some(Status::Warn));

        let bare: Info = serde_json::from_str(r#"{"ip":"2001:db8::1","bogon":true}"#).unwrap();
        let v = view(&bare, None);
        assert_eq!(v.summary, "2001:db8::1");
        assert!(parts(&v).1.is_empty());
    }

    #[tokio::test]
    async fn config_errors() {
        assert!(
            IpInfo::default()
                .init(serde_json::json!({ "uri": "x" }))
                .await
                .is_err()
        );
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
