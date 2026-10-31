//! Google Antigravity model quotas, from the language server the Antigravity
//! app runs on this machine. The module finds that process, takes the CSRF
//! token from its command line and the port it listens on, and asks it for
//! `RetrieveUserQuotaSummary` (Connect JSON over local HTTPS with a
//! self-signed certificate). The token is used for that one request and never
//! stored or shown.
//!
//! This only works while the app is open. When it is not, the tile says so
//! instead of failing.
//!
//! Untested live: built from the app's own client code and a sample reply.

use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, Status, View, Widget};
use dashboard_host::Module;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;

use super::{Window, now_secs, worst};
use dashboard_modules_common::http::localhost_client;
use dashboard_modules_common::manifest;

const SITE: &str = "https://antigravity.google";
const PATH: &str = "/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary";

#[derive(Default)]
pub struct Antigravity;

#[async_trait]
impl Module for Antigravity {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() && config != json!({}) {
            bail!("the antigravity module has no settings");
        }
        Ok(manifest(
            "antigravity",
            "Antigravity",
            120_000,
            15_000,
            Size::Medium,
            44,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let Some(server) = find_server().await? else {
            return Ok(not_running());
        };
        let reply = ask(&server).await?;
        Ok(view(&reply))
    }
}

/// The running language server: its pid and CSRF token.
struct Server {
    pid: u32,
    csrf: String,
}

/// The Antigravity language server among the running processes, if any.
async fn find_server() -> Result<Option<Server>> {
    let out = Command::new("ps")
        .args(["-axww", "-o", "pid=,args="])
        .stdin(Stdio::null())
        .output()
        .await
        .context("running ps")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(server_from_ps_line))
}

/// `1234 /…/language_server_macos_arm --csrf_token X … --app_data_dir antigravity`.
fn server_from_ps_line(line: &str) -> Option<Server> {
    let mut words = line.split_whitespace();
    let pid = words.next()?.parse().ok()?;
    let args: Vec<&str> = words.collect();
    // The executable path may hold spaces, so look at every word before
    // the first flag.
    if !args
        .iter()
        .take_while(|a| !a.starts_with("--"))
        .any(|a| a.contains("language_server"))
    {
        return None;
    }
    let flag = |name: &str| {
        args.iter()
            .position(|a| *a == name)
            .and_then(|i| args.get(i + 1))
            .map(|v| v.to_string())
    };
    flag("--app_data_dir").filter(|d| d.starts_with("antigravity"))?;
    Some(Server {
        pid,
        csrf: flag("--csrf_token")?,
    })
}

/// TCP ports the process listens on.
async fn ports(pid: u32) -> Result<Vec<u16>> {
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-Fn", "-p"])
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .output()
        .await
        .context("running lsof")?;
    Ok(parse_lsof(&String::from_utf8_lossy(&out.stdout)))
}

/// `-Fn` output: `n127.0.0.1:53210` lines.
fn parse_lsof(text: &str) -> Vec<u16> {
    let mut ports: Vec<u16> = text
        .lines()
        .filter_map(|l| l.strip_prefix('n')?.rsplit(':').next()?.parse().ok())
        .collect();
    ports.dedup();
    ports
}

/// Tries each port the server listens on until one answers the quota call.
async fn ask(server: &Server) -> Result<Reply> {
    let client = localhost_client()?;
    let ports = ports(server.pid).await?;
    if ports.is_empty() {
        bail!("the Antigravity language server listens on no TCP port");
    }
    let mut last = None;
    for port in ports {
        for scheme in ["https", "http"] {
            let sent = client
                .post(format!("{scheme}://127.0.0.1:{port}{PATH}"))
                .header("Content-Type", "application/json")
                .header("Connect-Protocol-Version", "1")
                .header("x-codeium-csrf-token", &server.csrf)
                .body("{}")
                .send()
                .await;
            match sent {
                Ok(r) if r.status().is_success() => {
                    return r.json().await.context("reading the Antigravity quota");
                }
                Ok(r) => last = Some(format!("HTTP {} on port {port}", r.status())),
                Err(_) => {}
            }
        }
    }
    bail!(
        "the Antigravity language server did not answer the quota call ({})",
        last.unwrap_or_else(|| "no port answered".into())
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Reply {
    response: Summary,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Summary {
    description: Option<String>,
    groups: Vec<Group>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Group {
    display_name: Option<String>,
    buckets: Vec<Bucket>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Bucket {
    display_name: Option<String>,
    /// 0 to 1. Missing when the server has no number for it.
    remaining_fraction: Option<f64>,
    /// "Refreshes in 3h 12m".
    description: Option<String>,
    disabled: bool,
}

fn not_running() -> View {
    View {
        status: Status::Unknown,
        summary: "not running".into(),
        root: Widget::Column {
            children: vec![
                Widget::Text {
                    text: "Antigravity is not running. Its quota can only be read while the \
                           app is open."
                        .into(),
                    markdown: false,
                },
                super::link("Antigravity", "antigravity.google", SITE),
            ],
        },
    }
}

fn view(reply: &Reply) -> View {
    let groups = &reply.response.groups;
    let mut windows = vec![];
    for g in groups {
        for b in &g.buckets {
            let (Some(name), Some(left)) = (&b.display_name, b.remaining_fraction) else {
                continue;
            };
            if b.disabled {
                continue;
            }
            let label = match (&g.display_name, groups.len() > 1) {
                (Some(group), true) if group != name => format!("{group} · {name}"),
                _ => name.clone(),
            };
            windows.push(Window {
                label,
                used_pct: (100.0 * (1.0 - left)).clamp(0.0, 100.0),
                resets_at: None,
                length: None,
                note: b.description.clone().filter(|d| !d.is_empty()),
            });
        }
    }
    let now = now_secs();
    let mut children: Vec<Widget> = windows.iter().map(|w| w.gauge(now)).collect();
    if let Some(d) = &reply.response.description
        && !d.is_empty()
    {
        children.push(Widget::Text {
            text: d.clone(),
            markdown: false,
        });
    }
    if children.is_empty() {
        children.push(Widget::Text {
            text: "No quotas reported.".into(),
            markdown: false,
        });
    }
    children.push(super::link("Antigravity", "antigravity.google", SITE));
    // The tightest quota is the one that matters.
    let summary = windows
        .iter()
        .max_by(|a, b| a.used_pct.total_cmp(&b.used_pct))
        .map(|w| format!("{:.0}% {}", w.used_pct, w.label))
        .unwrap_or_else(|| "no quotas".into());
    View {
        status: worst(&windows),
        summary,
        root: Widget::Column { children },
    }
}

fn sample() -> Reply {
    serde_json::from_value(json!({
        "response": {
            "groups": [
                { "displayName": "Gemini", "buckets": [
                    { "bucketId": "gemini-pro", "displayName": "Gemini 3 Pro",
                      "remainingFraction": 0.62, "description": "Refreshes in 3h 12m" },
                    { "bucketId": "gemini-flash", "displayName": "Gemini 3 Flash",
                      "remainingFraction": 0.95, "description": "Refreshes in 3h 12m" }
                ]},
                { "displayName": "Claude", "buckets": [
                    { "bucketId": "claude", "displayName": "Claude Sonnet 4.5",
                      "remainingFraction": 0.18, "description": "Refreshes in 1h 40m" }
                ]}
            ]
        }
    }))
    .expect("demo data parses")
}

pub fn catalog() -> CatalogEntry {
    CatalogEntry {
        description: "Google Antigravity model quotas, read from the app's local language \
                      server while Antigravity is open. Untested live."
            .into(),
        tags: crate::strings(&["ai", "agents", "usage", "google", "local"]),
        requirements: Requirements {
            binaries: crate::strings(&["ps", "lsof"]),
            env: vec![],
            hosts: vec![],
        },
        ..crate::entry("antigravity", "Antigravity", Size::Medium, view(&sample()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_server_in_ps_output() {
        let line = "  4242 /Applications/Antigravity.app/Contents/Resources/app/extensions/antigravity/bin/language_server_macos_arm --csrf_token abc-123 --extension_server_port 5555 --extension_server_csrf_token zzz --app_data_dir antigravity --subclient_type ide";
        let s = server_from_ps_line(line).unwrap();
        assert_eq!(s.pid, 4242);
        assert_eq!(s.csrf, "abc-123");
        // Windsurf runs the same server for another app: not ours.
        let other = line.replace("--app_data_dir antigravity", "--app_data_dir windsurf");
        assert!(server_from_ps_line(&other).is_none());
        assert!(server_from_ps_line("  12 /bin/zsh --csrf_token x").is_none());
        // A space in the install path.
        let spaced = line.replace("Antigravity.app", "Antigravity Beta.app");
        assert_eq!(server_from_ps_line(&spaced).unwrap().csrf, "abc-123");
    }

    #[test]
    fn reads_lsof_ports() {
        assert_eq!(
            parse_lsof("p4242\nf12\nn127.0.0.1:53210\nf13\nn*:53211\n"),
            vec![53210, 53211]
        );
    }

    #[test]
    fn quota_gauges_and_not_running() {
        let v = view(&sample());
        assert_eq!(v.summary, "82% Claude · Claude Sonnet 4.5");
        assert_eq!(v.status, Status::Warn);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        assert_eq!(children.len(), 4);
        let Widget::Gauge { label, caption, .. } = &children[0] else {
            panic!()
        };
        assert_eq!(label, "Gemini · Gemini 3 Pro");
        assert_eq!(caption.as_deref(), Some("Refreshes in 3h 12m"));

        let empty: Reply = serde_json::from_value(json!({ "response": {} })).unwrap();
        assert_eq!(view(&empty).status, Status::Unknown);
        assert_eq!(not_running().summary, "not running");
    }
}
