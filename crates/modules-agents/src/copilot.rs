//! GitHub Copilot monthly quotas (premium requests and any other limited
//! quota), from the endpoint Copilot's own clients use. Signs in with the `gh`
//! CLI's token, like the GitHub modules.

use std::collections::BTreeMap;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View, Widget};
use dashboard_host::Module;
use serde::Deserialize;
use tokio::process::Command;

use super::{Window, now_secs, worst};
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::manifest;
use dashboard_modules_common::time::{demo_time, parse_time};

const URL: &str = "https://api.github.com/copilot_internal/user";

#[derive(Debug, Deserialize)]
struct User {
    copilot_plan: Option<String>,
    /// `YYYY-MM-DD`
    quota_reset_date: Option<String>,
    #[serde(default)]
    quota_snapshots: BTreeMap<String, Quota>,
}

#[derive(Debug, Deserialize)]
struct Quota {
    #[serde(default)]
    unlimited: bool,
    #[serde(default)]
    entitlement: f64,
    #[serde(default)]
    remaining: f64,
    percent_remaining: Option<f64>,
}

#[derive(Default)]
pub struct Copilot;

#[async_trait]
impl Module for Copilot {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() && config != serde_json::json!({}) {
            bail!("the copilot module has no settings");
        }
        Ok(manifest(
            "copilot",
            "Copilot",
            300_000,
            60_000,
            Size::Medium,
            44,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let token = gh_token().await?;
        let resp = client()?
            .get(URL)
            .header("Authorization", format!("token {token}"))
            // The endpoint answers editor clients; these match Copilot Chat.
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header("X-Github-Api-Version", "2025-04-01")
            .send()
            .await
            .context("asking GitHub for Copilot usage")?;
        let user: User = check(resp)
            .await?
            .json()
            .await
            .context("reading Copilot usage")?;
        Ok(view(&user, now_secs()))
    }
}

async fn gh_token() -> Result<String> {
    let out = Command::new("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .output()
        .await
        .context("running gh (is the GitHub CLI installed?)")?;
    if !out.status.success() {
        bail!(
            "gh auth token: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn view(user: &User, now: i64) -> View {
    let reset = user.quota_reset_date.as_deref();
    let resets_at = reset.and_then(|d| parse_time(&format!("{d}T00:00:00Z")));
    let starts_at = reset
        .and_then(month_before)
        .and_then(|d| parse_time(&format!("{d}T00:00:00Z")));
    let length = resets_at.zip(starts_at).map(|(r, s)| r - s);

    let mut windows = vec![];
    let mut unlimited = vec![];
    for (id, q) in &user.quota_snapshots {
        if q.unlimited {
            unlimited.push(quota_name(id));
            continue;
        }
        if q.entitlement <= 0.0 {
            continue;
        }
        let left = q
            .percent_remaining
            .unwrap_or(100.0 * q.remaining / q.entitlement);
        windows.push(Window {
            label: quota_name(id),
            used_pct: (100.0 - left).clamp(0.0, 100.0),
            resets_at,
            length,
            note: Some(format!("{:.0} of {:.0} left", q.remaining, q.entitlement)),
        });
    }

    let mut children: Vec<Widget> = windows.iter().map(|w| w.gauge(now)).collect();
    if !unlimited.is_empty() {
        children.push(Widget::Text {
            text: format!("Unlimited: {}", unlimited.join(", ").to_lowercase()),
            markdown: false,
        });
    }
    let plan = user.copilot_plan.as_deref().unwrap_or("unknown plan");
    let used = windows
        .iter()
        .map(|w| format!("{:.0}% {}", w.used_pct, w.label.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" · ");
    View {
        status: worst(&windows),
        summary: if used.is_empty() {
            plan.to_string()
        } else {
            format!("{plan} · {used}")
        },
        root: Widget::Column { children },
    }
}

fn quota_name(id: &str) -> String {
    match id {
        "premium_interactions" => "Premium requests".into(),
        "chat" => "Chat".into(),
        "completions" => "Completions".into(),
        other => other.replace('_', " "),
    }
}

pub fn catalog() -> CatalogEntry {
    // A sample reply in the endpoint's shape, resetting 12 days from now.
    let now = now_secs();
    let reset = demo_time(-12 * 86_400);
    let user: User = serde_json::from_value(serde_json::json!({
        "copilot_plan": "individual",
        "quota_reset_date": &reset[..10],
        "quota_snapshots": {
            "chat": { "unlimited": true, "entitlement": 0, "remaining": 0 },
            "completions": { "unlimited": true, "entitlement": 0, "remaining": 0 },
            "premium_interactions": { "unlimited": false, "entitlement": 300, "remaining": 117, "percent_remaining": 39.0 }
        }
    }))
    .expect("demo data parses");
    CatalogEntry {
        description: "GitHub Copilot monthly quotas, such as premium requests, and when they \
                      reset. Signs in with the gh CLI's login."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "github"]),
        requirements: Requirements {
            binaries: crate::strings(&["gh"]),
            env: vec![],
            hosts: crate::strings(&["api.github.com"]),
        },
        ..crate::entry("copilot", "Copilot", Size::Medium, view(&user, now))
    }
}

/// `2026-11-01` -> `2026-10-01`: the start of the monthly cycle.
fn month_before(date: &str) -> Option<String> {
    let (y, rest) = date.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    let (y, m): (i32, u32) = (y.parse().ok()?, m.parse().ok()?);
    let (y, m) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    Some(format!("{y:04}-{m:02}-{d}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_gauge_with_monthly_pace() {
        let user: User = serde_json::from_value(serde_json::json!({
            "copilot_plan": "individual",
            "quota_reset_date": "2026-11-01",
            "quota_snapshots": {
                "chat": { "unlimited": true, "entitlement": 0, "remaining": 0, "percent_remaining": 100.0 },
                "premium_interactions": { "unlimited": false, "entitlement": 1500, "remaining": 184, "percent_remaining": 12.3 }
            }
        }))
        .unwrap();
        let now = parse_time("2026-10-16T12:00:00Z").unwrap();
        let v = view(&user, now);
        assert_eq!(v.summary, "individual · 88% premium requests");
        assert_eq!(v.status, dashboard_contract::Status::Warn);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        let Widget::Gauge {
            marker, caption, ..
        } = &children[0]
        else {
            panic!()
        };
        assert_eq!(marker.map(|m| m.round()), Some(50.0));
        assert_eq!(
            caption.as_deref(),
            Some("184 of 1500 left · resets in 15d 12h")
        );
        assert!(matches!(&children[1], Widget::Text { text, .. } if text == "Unlimited: chat"));
    }

    #[test]
    fn cycle_start_wraps_the_year() {
        assert_eq!(month_before("2027-01-01").as_deref(), Some("2026-12-01"));
    }
}
