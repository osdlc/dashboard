//! Security settings of this machine, each on or off. No sudo needed.
//!
//! - macOS: the application firewall (`socketfilterfw --getglobalstate`),
//!   FileVault (`fdesetup status`), System Integrity Protection
//!   (`csrutil status`) and Gatekeeper (`spctl --status`).
//! - Linux, best effort: a firewall service (firewalld or ufw, through
//!   `systemctl is-active`), disk encryption (a `crypt` device in `lsblk`),
//!   SELinux (`getenforce`) or AppArmor, and Secure Boot (`mokutil`).
//!
//! A check whose command is missing or fails shows as unknown.

use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, ListItem, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

/// No settings yet; the block exists so a typo is an error.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    On,
    Off,
    Unknown,
}

/// One setting and what the machine says about it.
#[derive(Debug, Clone, PartialEq)]
struct Check {
    name: &'static str,
    state: State,
    /// The command's own words, e.g. "FileVault is On."
    detail: String,
}

impl Check {
    fn new(name: &'static str, found: Option<(State, String)>) -> Self {
        let (state, detail) = found.unwrap_or((State::Unknown, "could not check".into()));
        Self {
            name,
            state,
            detail,
        }
    }
}

#[derive(Default)]
pub struct Security;

#[async_trait]
impl Module for Security {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            let _: Config = serde_json::from_value(config)?;
        }
        Ok(manifest(
            "security",
            "Security",
            300_000,
            10_000,
            Size::Small,
            0,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let checks = if cfg!(target_os = "macos") {
            mac_checks().await
        } else if cfg!(target_os = "linux") {
            linux_checks().await
        } else {
            vec![]
        };
        Ok(view(&checks))
    }
}

async fn mac_checks() -> Vec<Check> {
    let firewall = output(
        "/usr/libexec/ApplicationFirewall/socketfilterfw",
        &["--getglobalstate"],
    )
    .await;
    vec![
        Check::new("Firewall", firewall.as_deref().and_then(parse_firewall)),
        Check::new(
            "FileVault",
            output("fdesetup", &["status"])
                .await
                .as_deref()
                .and_then(parse_filevault),
        ),
        Check::new(
            "System Integrity Protection",
            output("csrutil", &["status"])
                .await
                .as_deref()
                .and_then(parse_sip),
        ),
        Check::new(
            "Gatekeeper",
            output("spctl", &["--status"])
                .await
                .as_deref()
                .and_then(parse_gatekeeper),
        ),
    ]
}

async fn linux_checks() -> Vec<Check> {
    // Best effort: an active ufw service does not always mean `ufw status`
    // is active, and nftables or iptables used directly are not seen. With
    // neither service installed the state is unknown, not off.
    let mut firewall = None;
    for service in ["firewalld", "ufw"] {
        // `is-active` says `inactive` for units that do not exist, so check
        // that the unit is installed first.
        let load = output_any(
            "systemctl",
            &["show", "-p", "LoadState", "--value", service],
        )
        .await;
        if load.as_deref().map(str::trim) != Some("loaded") {
            continue;
        }
        // `is-active` exits non-zero when the service is not running, so read
        // stdout whatever the exit code.
        if let Some(out) = output_any("systemctl", &["is-active", service]).await {
            match out.trim() {
                "active" => {
                    firewall = Some((State::On, format!("{service} is active")));
                    break;
                }
                "" => {}
                _ => firewall = Some((State::Off, "no firewall service is active".into())),
            }
        }
    }
    let mac = match output("getenforce", &[]).await {
        Some(out) => parse_getenforce(&out),
        None => std::fs::read_to_string("/sys/module/apparmor/parameters/enabled")
            .ok()
            .map(|s| parse_apparmor(&s)),
    };
    vec![
        Check::new("Firewall", firewall),
        Check::new(
            "Disk encryption",
            output("lsblk", &["-rno", "TYPE"])
                .await
                .map(|out| parse_lsblk(&out)),
        ),
        Check::new("SELinux / AppArmor", mac),
        Check::new(
            "Secure Boot",
            output_any("mokutil", &["--sb-state"])
                .await
                .as_deref()
                .and_then(parse_secure_boot),
        ),
    ]
}

/// A command's stdout when it succeeds.
async fn output(program: &str, args: &[&str]) -> Option<String> {
    let (ok, out) = run(program, args).await?;
    ok.then_some(out)
}

/// A command's stdout and stderr, whatever its exit code.
async fn output_any(program: &str, args: &[&str]) -> Option<String> {
    run(program, args).await.map(|(_, out)| out)
}

async fn run(program: &str, args: &[&str]) -> Option<(bool, String)> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

/// The first non-empty line, trimmed.
fn first_line(out: &str) -> String {
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// "Firewall is enabled. (State = 1)"; state 2 blocks all incoming.
fn parse_firewall(out: &str) -> Option<(State, String)> {
    let line = first_line(out);
    let lower = line.to_lowercase();
    let state = if lower.contains("enabled") || lower.contains("block") {
        State::On
    } else if lower.contains("disabled") {
        State::Off
    } else {
        return None;
    };
    Some((state, line))
}

/// "FileVault is On." or "FileVault is Off.", maybe with progress lines after.
fn parse_filevault(out: &str) -> Option<(State, String)> {
    let line = first_line(out);
    let state = if line.contains("is On") {
        State::On
    } else if line.contains("is Off") {
        State::Off
    } else {
        return None;
    };
    // "Encryption in progress: Percent completed = 40" is worth showing.
    let detail = match out.lines().map(str::trim).nth(1) {
        Some(more) if !more.is_empty() => format!("{line} {more}"),
        _ => line,
    };
    Some((state, detail))
}

/// "System Integrity Protection status: enabled." A custom configuration
/// (some protections off) counts as off.
fn parse_sip(out: &str) -> Option<(State, String)> {
    let line = first_line(out);
    let (_, status) = line.split_once("status:")?;
    let status = status.trim().trim_end_matches('.');
    let state = match status {
        "enabled" => State::On,
        s if s.starts_with("disabled") || s.contains("Custom Configuration") => State::Off,
        _ => State::Unknown,
    };
    Some((state, format!("SIP {status}")))
}

/// "assessments enabled" or "assessments disabled".
fn parse_gatekeeper(out: &str) -> Option<(State, String)> {
    let line = first_line(out);
    let state = match line.as_str() {
        "assessments enabled" => State::On,
        "assessments disabled" => State::Off,
        _ => return None,
    };
    Some((state, line))
}

/// Any `crypt` device (LUKS, dm-crypt) counts as on.
fn parse_lsblk(out: &str) -> (State, String) {
    let n = out.lines().filter(|l| l.trim() == "crypt").count();
    match n {
        0 => (State::Off, "no encrypted volumes".into()),
        1 => (State::On, "1 encrypted volume".into()),
        n => (State::On, format!("{n} encrypted volumes")),
    }
}

/// "Enforcing", "Permissive" or "Disabled".
fn parse_getenforce(out: &str) -> Option<(State, String)> {
    let line = first_line(out);
    let state = match line.as_str() {
        "Enforcing" => State::On,
        "Permissive" | "Disabled" => State::Off,
        _ => return None,
    };
    Some((state, format!("SELinux {}", line.to_lowercase())))
}

/// `/sys/module/apparmor/parameters/enabled`: "Y" or "N".
fn parse_apparmor(out: &str) -> (State, String) {
    if out.trim() == "Y" {
        (State::On, "AppArmor enabled".into())
    } else {
        (State::Off, "AppArmor disabled".into())
    }
}

/// "SecureBoot enabled" or "SecureBoot disabled"; on a machine without EFI
/// mokutil says so, which is unknown.
fn parse_secure_boot(out: &str) -> Option<(State, String)> {
    let line = first_line(out);
    let state = match line.as_str() {
        "SecureBoot enabled" => State::On,
        "SecureBoot disabled" => State::Off,
        _ => return None,
    };
    Some((state, line))
}

/// The tile: one row per check. Anything off makes the tile a warning.
fn view(checks: &[Check]) -> View {
    if checks.is_empty() {
        return View {
            status: Status::Unknown,
            summary: "no checks on this system".into(),
            root: Widget::Text {
                text: "Security checks are available on macOS and Linux.".into(),
                markdown: false,
            },
        };
    }
    let off = checks.iter().filter(|c| c.state == State::Off).count();
    let unknown = checks.iter().filter(|c| c.state == State::Unknown).count();
    let on = checks.len() - off - unknown;
    let items = checks
        .iter()
        .map(|c| ListItem {
            tone: None,
            title: c.name.into(),
            subtitle: Some(c.detail.clone()),
            status: Some(match c.state {
                State::On => Status::Ok,
                State::Off => Status::Warn,
                State::Unknown => Status::Unknown,
            }),
            tags: vec![
                match c.state {
                    State::On => "on",
                    State::Off => "off",
                    State::Unknown => "unknown",
                }
                .into(),
            ],
            action: None,
        })
        .collect();
    let mut summary = format!("{on} of {} on", checks.len());
    if off > 0 {
        summary.push_str(&format!(" · {off} off"));
    }
    let status = if off > 0 {
        Status::Warn
    } else if on == 0 {
        Status::Unknown
    } else {
        Status::Ok
    };
    View {
        status,
        summary,
        root: Widget::List {
            items,
            empty_text: None,
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let demo = vec![
        Check::new(
            "Firewall",
            parse_firewall("Firewall is disabled. (State = 0)\n"),
        ),
        Check::new("FileVault", parse_filevault("FileVault is On.\n")),
        Check::new(
            "System Integrity Protection",
            parse_sip("System Integrity Protection status: enabled.\n"),
        ),
        Check::new("Gatekeeper", parse_gatekeeper("assessments enabled\n")),
    ];
    CatalogEntry {
        description: "Whether the firewall, disk encryption and system protections are on. \
                      macOS: firewall, FileVault, SIP and Gatekeeper. Linux (best effort): \
                      firewall service, encrypted volumes, SELinux or AppArmor, Secure Boot."
            .into(),
        tags: crate::strings(&["security", "local", "system"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("security", "Security", Size::Small, view(&demo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_output() {
        assert_eq!(
            parse_firewall("Firewall is enabled. (State = 1)\n")
                .unwrap()
                .0,
            State::On
        );
        assert_eq!(
            parse_firewall(
                "Firewall is blocking all non-essential incoming connections. (State = 2)"
            )
            .unwrap()
            .0,
            State::On
        );
        assert_eq!(
            parse_firewall("Firewall is disabled. (State = 0)")
                .unwrap()
                .0,
            State::Off
        );
        assert_eq!(parse_firewall(""), None);

        assert_eq!(
            parse_filevault("FileVault is Off.\n").unwrap(),
            (State::Off, "FileVault is Off.".into())
        );
        assert_eq!(
            parse_filevault("FileVault is On.\nEncryption in progress: Percent completed = 40\n")
                .unwrap(),
            (
                State::On,
                "FileVault is On. Encryption in progress: Percent completed = 40".into()
            )
        );

        assert_eq!(
            parse_sip("System Integrity Protection status: enabled.\n").unwrap(),
            (State::On, "SIP enabled".into())
        );
        let custom = "System Integrity Protection status: unknown (Custom Configuration).\n\n\
                      Configuration:\n\tApple Internal: disabled\n";
        assert_eq!(parse_sip(custom).unwrap().0, State::Off);
        assert_eq!(
            parse_sip("System Integrity Protection status: disabled.")
                .unwrap()
                .0,
            State::Off
        );

        assert_eq!(
            parse_gatekeeper("assessments disabled\n").unwrap().0,
            State::Off
        );
    }

    #[test]
    fn parses_linux_output() {
        let lsblk = "disk\npart\npart\ncrypt\nlvm\nlvm\n";
        assert_eq!(parse_lsblk(lsblk), (State::On, "1 encrypted volume".into()));
        assert_eq!(parse_lsblk("disk\npart\n").0, State::Off);
        assert_eq!(
            parse_getenforce("Enforcing\n").unwrap(),
            (State::On, "SELinux enforcing".into())
        );
        assert_eq!(parse_getenforce("Permissive").unwrap().0, State::Off);
        assert_eq!(parse_apparmor("Y\n").0, State::On);
        assert_eq!(
            parse_secure_boot("SecureBoot enabled\n").unwrap().0,
            State::On
        );
        assert_eq!(
            parse_secure_boot("This system doesn't support Secure Boot\n"),
            None
        );
    }

    #[test]
    fn view_counts_and_flags() {
        let v = catalog().demo;
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.summary, "3 of 4 on · 1 off");
        let Widget::List { items, .. } = &v.root else {
            panic!("list")
        };
        assert_eq!(items[0].status, Some(Status::Warn));
        assert_eq!(items[1].status, Some(Status::Ok));

        let unknown = [Check::new("Firewall", None)];
        let v = view(&unknown);
        assert_eq!(v.status, Status::Unknown);
        assert_eq!(view(&[]).status, Status::Unknown);
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
