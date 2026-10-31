//! Stdio plugin: shows Claude accounts from `cswap list --json`.
//! Speaks JSON-RPC 2.0, one message per line (see docs/contract.md).

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::process::Command;

use anyhow::{Context, Result, bail};
use dashboard_contract::rpc::{InitializeResult, Message, Response, RpcError, error_code, method};
use dashboard_contract::{
    Action, CONTRACT_VERSION, CatalogEntry, Cell, Column, CommandDecl, EntryKind, LayoutHints,
    Manifest, ParamDecl, Refresh, Requirements, Row, Size, Status, Thresholds, View, Widget,
};
use serde::Deserialize;

const THRESHOLDS: Thresholds = Thresholds {
    warn: 75.0,
    error: 90.0,
};

/// Serve the plugin protocol on stdin/stdout until stdin closes.
pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Message = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(err) => {
                eprintln!("cswap plugin: bad message: {err}");
                continue;
            }
        };
        match msg {
            Message::Request(req) => {
                let result = match req.method.as_str() {
                    method::INITIALIZE => to_value(InitializeResult {
                        manifest: manifest(),
                    }),
                    method::VIEW_RENDER => render().and_then(to_value),
                    method::SHUTDOWN => Ok(serde_json::Value::Null),
                    other => Err(anyhow::anyhow!("unknown method {other}")),
                };
                let resp = match result {
                    Ok(value) => Response {
                        jsonrpc: "2.0".into(),
                        id: req.id,
                        result: Some(value),
                        error: None,
                    },
                    Err(err) => Response {
                        jsonrpc: "2.0".into(),
                        id: req.id,
                        result: None,
                        error: Some(RpcError {
                            code: error_code::UPSTREAM_UNAVAILABLE,
                            message: format!("{err:#}"),
                            data: None,
                        }),
                    },
                };
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
            }
            Message::Notification(n) if n.method == method::EXIT => break,
            _ => {}
        }
    }
    Ok(())
}

fn to_value<T: serde::Serialize>(v: T) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(v)?)
}

fn manifest() -> Manifest {
    Manifest {
        id: "cswap".into(),
        name: "Claude accounts".into(),
        contract_version: CONTRACT_VERSION.into(),
        refresh: Refresh::Poll {
            interval_ms: 60_000,
            min_interval_ms: Some(15_000),
        },
        layout: LayoutHints {
            size: Size::Medium,
            priority: 50,
            min_size: Some(Size::Small),
        },
        commands: vec![CommandDecl {
            id: "switch".into(),
            label: "Switch to account".into(),
            argv: vec!["cswap".into(), "switch".into(), "{account}".into()],
            cwd: None,
            params: vec![ParamDecl {
                name: "account".into(),
                pattern: "^[0-9]+$".into(),
            }],
        }],
        config_schema: None,
    }
}

/// The catalog entry, with a demo made from a sample `cswap list --json`.
pub fn catalog() -> CatalogEntry {
    let list: CswapList = serde_json::from_value(serde_json::json!({ "accounts": [
        { "number": 1, "email": "me@example.com", "active": true, "usage": {
            "fiveHour": { "pct": 42.0, "countdown": "2h 10m", "expectedPct": 57.0, "aheadOfPace": false },
            "sevenDay": { "pct": 63.0, "expectedPct": 55.0, "aheadOfPace": true, "willLastToReset": true }
        }},
        { "number": 2, "email": "work@example.com", "active": false, "usage": {
            "fiveHour": { "pct": 91.0, "countdown": "38m", "expectedPct": 87.0, "aheadOfPace": true },
            "sevenDay": { "pct": 78.0, "expectedPct": 60.0, "aheadOfPace": true, "willLastToReset": false }
        }}
    ]}))
    .expect("demo data parses");
    let m = manifest();
    CatalogEntry {
        id: m.id,
        name: m.name,
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Claude Code accounts and how much of each 5-hour and weekly limit is \
                      used, from cswap. Click an account to switch to it."
            .into(),
        tags: ["ai", "agents", "usage", "claude"]
            .map(String::from)
            .to_vec(),
        kind: EntryKind::Plugin,
        source: "bundled".into(),
        default_size: m.layout.size,
        config_schema: None,
        requirements: Requirements {
            binaries: vec!["cswap".into()],
            env: vec![],
            hosts: vec![],
        },
        demo: build_view(&list),
    }
}

// ---- cswap list --json (only the fields used) ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CswapList {
    accounts: Vec<Account>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    number: u32,
    email: String,
    active: bool,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    five_hour: Option<Window>,
    seven_day: Option<Window>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    pct: f64,
    countdown: Option<String>,
    expected_pct: Option<f64>,
    ahead_of_pace: Option<bool>,
    will_last_to_reset: Option<bool>,
}

fn render() -> Result<View> {
    let out = cswap(&["list", "--json"]).context("running cswap")?;
    if !out.status.success() {
        bail!(
            "cswap list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let list: CswapList =
        serde_json::from_slice(&out.stdout).context("parsing cswap list --json")?;
    Ok(build_view(&list))
}

/// Run `cswap`. On Windows it may be a `.cmd` shim (an npm-style install),
/// which only runs when named with its extension; and as a Python tool it
/// writes in the console's code page unless told to use UTF-8.
fn cswap(args: &[&str]) -> std::io::Result<std::process::Output> {
    let run = |program: &str| {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if cfg!(windows) {
            cmd.env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
        }
        cmd.output()
    };
    match run("cswap") {
        Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound => run("cswap.cmd"),
        other => other,
    }
}

fn build_view(list: &CswapList) -> View {
    let columns = [
        ("account", "Account"),
        ("five_hour", "5 hours"),
        ("seven_day", "7 days"),
        ("resets", "5h resets"),
    ]
    .map(|(key, label)| Column {
        key: key.into(),
        label: label.into(),
    })
    .to_vec();

    let rows: Vec<Row> = list.accounts.iter().map(account_row).collect();
    let active = list.accounts.iter().zip(&rows).find(|(a, _)| a.active);

    let (status, summary) = match active {
        Some((a, row)) => {
            let five = a
                .usage
                .as_ref()
                .and_then(|u| u.five_hour.as_ref())
                .map(|w| w.pct);
            let five = five
                .map(|p| format!(" at {p:.0}% (5h)"))
                .unwrap_or_default();
            (
                row.status.unwrap_or(Status::Unknown),
                format!("{} accounts · active #{}{five}", rows.len(), a.number),
            )
        }
        None => (
            Status::Unknown,
            format!("{} accounts · none active", rows.len()),
        ),
    };

    View {
        status,
        summary,
        root: Widget::Table { columns, rows },
    }
}

fn account_row(a: &Account) -> Row {
    let five = a.usage.as_ref().and_then(|u| u.five_hour.as_ref());
    let seven = a.usage.as_ref().and_then(|u| u.seven_day.as_ref());

    let mut cells = BTreeMap::new();
    let marker = if a.active { "● " } else { "  " };
    cells.insert(
        "account".into(),
        Cell::Text(format!("{marker}{} {}", a.number, a.email)),
    );
    if let Some(w) = five {
        cells.insert("five_hour".into(), Cell::Widget(gauge("5h", w)));
        cells.insert(
            "resets".into(),
            Cell::Text(w.countdown.clone().unwrap_or_default()),
        );
    }
    if let Some(w) = seven {
        cells.insert("seven_day".into(), Cell::Widget(gauge("7d", w)));
    }

    let worst = five
        .iter()
        .chain(seven.iter())
        .map(|w| w.pct)
        .fold(0.0, f64::max);
    let mut status = if worst >= THRESHOLDS.error {
        Status::Error
    } else if worst >= THRESHOLDS.warn {
        Status::Warn
    } else {
        Status::Ok
    };
    if status == Status::Ok && seven.and_then(|w| w.will_last_to_reset) == Some(false) {
        status = Status::Warn;
    }

    let action = (!a.active).then(|| Action::RunCommand {
        command: "switch".into(),
        params: BTreeMap::from([("account".into(), a.number.to_string())]),
    });

    Row {
        cells,
        status: Some(status),
        action,
    }
}

fn gauge(label: &str, w: &Window) -> Widget {
    let caption = match (w.ahead_of_pace, w.will_last_to_reset) {
        (_, Some(false)) => Some("won't last".to_string()),
        (Some(true), _) => Some("ahead of pace".to_string()),
        (Some(false), _) => Some("on pace".to_string()),
        _ => None,
    };
    Widget::Gauge {
        label: label.into(),
        percent: w.pct,
        marker: w.expected_pct,
        thresholds: Some(THRESHOLDS),
        caption,
    }
}
