//! Cursor usage this billing cycle: included usage of the plan and any
//! on-demand spend, from the endpoint Cursor's own dashboard uses
//! (`cursor.com/api/usage-summary`). Signs in with the token the Cursor app
//! keeps in its local state database (`cursorAuth/accessToken`), read with
//! the `sqlite3` CLI at refresh time and never stored or shown.
//!
//! Checked live once against a free account. The paid-plan fields (included
//! usage, on-demand limit) come from a sample reply and are untested live.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, Status, View, Widget};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

use super::{Window, now_secs};
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::time::{demo_time, parse_time};
use dashboard_modules_common::{expand_home, manifest};

const URL: &str = "https://cursor.com/api/usage-summary";
const DASHBOARD: &str = "https://cursor.com/dashboard?tab=usage";

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Cursor's `state.vscdb`; default the app's own, per OS.
    db: Option<String>,
}

#[derive(Default)]
pub struct Cursor {
    db: PathBuf,
}

#[async_trait]
impl Module for Cursor {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        let config: Config = if config.is_null() {
            Config::default()
        } else {
            serde_json::from_value(config)?
        };
        self.db = match config.db {
            Some(p) => expand_home(&p),
            None => default_db(),
        };
        Ok(manifest(
            "cursor",
            "Cursor",
            300_000,
            60_000,
            Size::Medium,
            44,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let token = access_token(&self.db).await?;
        let user = user_id(&token).context("reading the Cursor token")?;
        let resp = client()?
            .get(URL)
            // The dashboard's session cookie is `<user id>::<access token>`.
            .header(
                "Cookie",
                format!("WorkosCursorSessionToken={user}%3A%3A{token}"),
            )
            .send()
            .await
            .context("asking Cursor for usage")?;
        let summary: Summary = check(resp)
            .await?
            .json()
            .await
            .context("reading Cursor usage")?;
        Ok(view(&summary, now_secs()))
    }
}

fn default_db() -> PathBuf {
    let rel = "Cursor/User/globalStorage/state.vscdb";
    if cfg!(target_os = "macos") {
        expand_home("~/Library/Application Support").join(rel)
    } else if cfg!(windows) {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(rel)
    } else {
        expand_home("~/.config").join(rel)
    }
}

/// The access token from Cursor's state database, opened read-only.
async fn access_token(db: &Path) -> Result<String> {
    if !db.is_file() {
        bail!(
            "no Cursor state database at {} (is Cursor installed and signed in?)",
            db.display()
        );
    }
    let out = Command::new("sqlite3")
        .arg("-readonly")
        .arg(db)
        .arg("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'")
        .stdin(Stdio::null())
        .output()
        .await
        .context("running sqlite3")?;
    if !out.status.success() {
        bail!(
            "sqlite3 could not read the Cursor database: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if token.is_empty() {
        bail!("Cursor is not signed in (no access token in its database)");
    }
    Ok(token)
}

/// The user id inside the token (a JWT): the part of `sub` after any `|`.
fn user_id(token: &str) -> Result<String> {
    let payload = token.split('.').nth(1).context("not a JWT")?;
    let claims: Value = serde_json::from_slice(&base64url(payload).context("not a JWT")?)?;
    let sub = claims["sub"].as_str().context("no user in the token")?;
    Ok(sub.rsplit('|').next().unwrap_or(sub).to_string())
}

fn base64url(s: &str) -> Option<Vec<u8>> {
    let mut out = vec![];
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Summary {
    billing_cycle_start: Option<String>,
    billing_cycle_end: Option<String>,
    membership_type: Option<String>,
    is_unlimited: bool,
    individual_usage: Option<Individual>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Individual {
    plan: Option<Bucket>,
    on_demand: Option<Bucket>,
}

/// Amounts are in US cents.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Bucket {
    enabled: bool,
    used: f64,
    limit: Option<f64>,
}

fn view(s: &Summary, now: i64) -> View {
    let plan = s.membership_type.as_deref().unwrap_or("unknown plan");
    let start = s.billing_cycle_start.as_deref().and_then(parse_time);
    let end = s.billing_cycle_end.as_deref().and_then(parse_time);
    let usage = s.individual_usage.as_ref();
    let included = usage
        .and_then(|u| u.plan.as_ref())
        .filter(|b| b.enabled && b.limit.is_some_and(|l| l > 0.0));
    let on_demand = usage
        .and_then(|u| u.on_demand.as_ref())
        .filter(|b| b.enabled);

    let mut windows = vec![];
    if let Some(b) = included {
        let limit = b.limit.unwrap_or(1.0);
        windows.push(Window {
            label: "Included usage".into(),
            used_pct: (100.0 * b.used / limit).clamp(0.0, 100.0),
            resets_at: end,
            length: end.zip(start).map(|(e, s)| e - s),
            note: Some(format!("{} of {}", dollars(b.used), dollars(limit))),
        });
    }
    if let Some(b) = on_demand
        && let Some(limit) = b.limit.filter(|l| *l > 0.0)
    {
        windows.push(Window {
            label: "On-demand".into(),
            used_pct: (100.0 * b.used / limit).clamp(0.0, 100.0),
            resets_at: end,
            length: end.zip(start).map(|(e, s)| e - s),
            note: Some(format!("{} of {}", dollars(b.used), dollars(limit))),
        });
    }

    let mut children: Vec<Widget> = windows.iter().map(|w| w.gauge(now)).collect();
    let mut facts = vec![];
    if s.is_unlimited {
        facts.push("unlimited plan".to_string());
    } else if included.is_none() {
        // A free or canceled account: nothing included to measure.
        facts.push(format!("{plan} plan: no included usage this cycle"));
    }
    if let Some(b) = on_demand.filter(|b| b.limit.is_none_or(|l| l <= 0.0)) {
        facts.push(format!("on-demand {} (no limit)", dollars(b.used)));
    }
    if !facts.is_empty() {
        children.push(Widget::Text {
            text: facts.join(" · "),
            markdown: false,
        });
    }
    children.push(super::link("Usage dashboard", "cursor.com", DASHBOARD));

    let used = windows
        .iter()
        .map(|w| format!("{:.0}% {}", w.used_pct, w.label.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" · ");
    View {
        status: if windows.is_empty() {
            Status::Ok
        } else {
            super::worst(&windows)
        },
        summary: if used.is_empty() {
            plan.to_string()
        } else {
            format!("{plan} · {used}")
        },
        root: Widget::Column { children },
    }
}

fn dollars(cents: f64) -> String {
    format!("${:.2}", cents / 100.0)
}

/// A sample reply in the endpoint's shape: a Pro plan 18 days into its cycle.
fn sample() -> Summary {
    let start = demo_time(18 * 86_400);
    let end = demo_time(-12 * 86_400);
    serde_json::from_value(serde_json::json!({
        "billingCycleStart": start,
        "billingCycleEnd": end,
        "membershipType": "pro",
        "limitType": "user",
        "isUnlimited": false,
        "individualUsage": {
            "plan": { "enabled": true, "used": 1240, "limit": 2000, "remaining": 760,
                      "breakdown": { "included": 1240, "bonus": 0, "total": 1240 } },
            "onDemand": { "enabled": true, "used": 310, "limit": 5000, "remaining": 4690 }
        },
        "teamUsage": {}
    }))
    .expect("demo data parses")
}

pub fn catalog() -> CatalogEntry {
    CatalogEntry {
        description: "Cursor usage this billing cycle: included usage and on-demand spend. \
                      Signs in with the Cursor app's own login. Paid plans untested live."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "cursor"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["sqlite3"]),
            env: vec![],
            hosts: crate::strings(&["cursor.com"]),
        },
        ..crate::entry(
            "cursor",
            "Cursor",
            Size::Medium,
            view(&sample(), now_secs()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn included_and_on_demand_gauges() {
        let s: Summary = serde_json::from_value(serde_json::json!({
            "billingCycleStart": "2026-10-12T00:00:00.000Z",
            "billingCycleEnd": "2026-11-12T00:00:00.000Z",
            "membershipType": "pro",
            "isUnlimited": false,
            "individualUsage": {
                "plan": { "enabled": true, "used": 1600, "limit": 2000, "remaining": 400 },
                "onDemand": { "enabled": true, "used": 500, "limit": 5000, "remaining": 4500 }
            }
        }))
        .unwrap();
        let now = parse_time("2026-10-27T12:00:00Z").unwrap();
        let v = view(&s, now);
        assert_eq!(v.summary, "pro · 80% included usage · 10% on-demand");
        assert_eq!(v.status, Status::Warn);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        let Widget::Gauge { caption, .. } = &children[0] else {
            panic!()
        };
        assert_eq!(
            caption.as_deref(),
            Some("$16.00 of $20.00 · resets in 15d 12h")
        );
        assert!(matches!(children.last(), Some(Widget::List { .. })));
    }

    #[test]
    fn free_or_canceled_account() {
        let s: Summary = serde_json::from_value(serde_json::json!({
            "membershipType": "free",
            "isUnlimited": false,
            "individualUsage": {
                "plan": { "enabled": false, "used": 0, "limit": 0, "remaining": 0 },
                "onDemand": { "enabled": false, "used": 0, "limit": null, "remaining": null }
            }
        }))
        .unwrap();
        let v = view(&s, 0);
        assert_eq!(v.summary, "free");
        assert_eq!(v.status, Status::Ok);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        assert!(
            matches!(&children[0], Widget::Text { text, .. } if text == "free plan: no included usage this cycle")
        );
    }

    #[test]
    fn user_id_from_a_jwt() {
        // {"sub":"auth0|user_01ABC","exp":1}, no signature check needed.
        let token = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhdXRoMHx1c2VyXzAxQUJDIiwiZXhwIjoxfQ.sig";
        assert_eq!(user_id(token).unwrap(), "user_01ABC");
        assert!(user_id("nope").is_err());
    }
}
