//! `dashboard plugin-check -- <program> [args]`: run a stdio plugin the way
//! the dashboard does, check what it sends back, and print its tile as text.

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use dashboard_cli_kit::Failure;
use dashboard_contract::check::{Finding, check_manifest, check_view};
use dashboard_contract::{Refresh, TileUpdate};
use dashboard_host::{History, Module, Plugin};

pub struct Options {
    pub config: Option<String>,
    pub renders: u32,
    pub wait_ms: Option<u64>,
    pub width: u16,
    pub height: Option<u16>,
    /// Print one `plugin-check` document instead of the report.
    pub json: bool,
    pub argv: Vec<String>,
}

/// Prints a report; fails when the plugin fails or sends something wrong.
pub async fn run(o: Options) -> Result<()> {
    let config: serde_json::Value = match &o.config {
        Some(text) => serde_json::from_str(text)
            .map_err(|e| Failure::usage(format!("--plugin-config must be JSON: {e}")))?,
        None => serde_json::Value::Null,
    };
    // The text report, left out under --json.
    let say = |line: String| {
        if !o.json {
            println!("{line}");
        }
    };
    say(format!("plugin    {}", o.argv.join(" ")));

    let mut plugin = Plugin::new("check", o.argv.clone());
    let started = Instant::now();
    let manifest = match plugin.init(config).await {
        Ok(m) => m,
        Err(err) if io_kind(&err) == Some(ErrorKind::NotFound) => {
            return Err(Failure::not_installed(format!(
                "couldn't start the plugin: {} was not found.",
                o.argv[0]
            ))
            .hint("Check the program name and PATH.")
            .into());
        }
        Err(err) if io_kind(&err) == Some(ErrorKind::PermissionDenied) => {
            return Err(Failure::new(
                "not-executable",
                format!(
                    "couldn't start the plugin: {} is not executable.",
                    o.argv[0]
                ),
            )
            .hint(format!("Run `chmod +x {}`.", o.argv[0]))
            .into());
        }
        Err(err) => return Err(err.context("initialize failed")),
    };
    let interval = match manifest.refresh {
        Refresh::Poll { interval_ms, .. } => interval_ms,
        Refresh::Push => 0,
    };
    say(format!(
        "manifest  {} {:?} · every {} · size {:?} · answered in {} ms",
        manifest.id,
        manifest.name,
        ms(interval),
        manifest.layout.size,
        started.elapsed().as_millis()
    ));
    let mut findings: Vec<Finding> = check_manifest(&manifest)
        .into_iter()
        .map(|f| Finding {
            text: format!("manifest: {}", f.text),
            ..f
        })
        .collect();

    // Short waits by default, so charts that keep history get a few points.
    let wait = Duration::from_millis(o.wait_ms.unwrap_or(interval.min(1000)));
    let mut history = History::default();
    let mut last = None;
    let mut seen = std::collections::HashSet::new();
    let mut renders = vec![];
    for i in 1..=o.renders.max(1) {
        if i > 1 {
            tokio::time::sleep(wait).await;
        }
        let started = Instant::now();
        let mut view = plugin
            .render()
            .await
            .with_context(|| format!("view/render {i} failed"))?;
        let took = started.elapsed().as_millis();
        say(format!(
            "render {i}  {:?} · {:?} · {took} ms",
            view.status, view.summary
        ));
        renders.push(serde_json::json!({
            "status": view.status,
            "summary": view.summary,
            "ms": took,
        }));
        for f in check_view(&view) {
            // The same finding on every render is reported once, from the first.
            if seen.insert(f.text.clone()) {
                findings.push(Finding {
                    text: format!("render {i}: {}", f.text),
                    ..f
                });
            }
        }
        history.apply("check", &mut view);
        last = Some(view);
    }

    let view = last.expect("at least one render");
    let problems = findings.iter().filter(|f| f.problem).count();
    if o.json {
        crate::print_json(
            "plugin-check",
            &serde_json::json!({
                "plugin": o.argv,
                "manifest": manifest,
                "renders": renders,
                "view": view,
                "findings": findings,
            }),
        )?;
        return if problems == 0 {
            Ok(())
        } else {
            Err(Failure::new("problems", format!("{problems} problem(s)"))
                .after_document()
                .into())
        };
    }
    let tile = TileUpdate {
        tile_id: manifest.id.clone(),
        view,
        updated_at: dashboard_host::now_ms(),
        stale: false,
        demo: false,
    };
    let height = o
        .height
        .unwrap_or_else(|| dashboard_tui::tile_min_height(&tile, o.width).max(8));
    println!("\n{}\n", dashboard_tui::tile_text(&tile, o.width, height));

    for f in findings.iter().filter(|f| !f.problem) {
        println!("note      {}", f.text);
    }
    for f in findings.iter().filter(|f| f.problem) {
        println!("problem   {}", f.text);
    }
    if problems == 0 {
        println!("no problems found");
        Ok(())
    } else {
        Err(Failure::new("problems", format!("{problems} problem(s)")).into())
    }
}

/// Whether starting the plugin failed because its program doesn't exist.
/// The kind of the first io error in the chain, if any.
fn io_kind(err: &anyhow::Error) -> Option<ErrorKind> {
    err.chain()
        .find_map(|e| e.downcast_ref::<std::io::Error>())
        .map(std::io::Error::kind)
}

fn ms(n: u64) -> String {
    if n >= 1000 && n.is_multiple_of(1000) {
        format!("{}s", n / 1000)
    } else {
        format!("{n}ms")
    }
}
