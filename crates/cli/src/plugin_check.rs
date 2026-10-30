//! `dashboard plugin-check -- <program> [args]`: run a stdio plugin the way
//! the dashboard does, check what it sends back, and print its tile as text.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use dashboard_contract::check::{Finding, check_manifest, check_view};
use dashboard_contract::{Refresh, TileUpdate};
use dashboard_host::{History, Module, Plugin};

pub struct Options {
    pub config: Option<String>,
    pub renders: u32,
    pub wait_ms: Option<u64>,
    pub width: u16,
    pub height: Option<u16>,
    pub json: bool,
    pub argv: Vec<String>,
}

/// Prints a report; fails when the plugin fails or sends something wrong.
pub async fn run(o: Options) -> Result<()> {
    let config: serde_json::Value = match &o.config {
        Some(text) => serde_json::from_str(text).context("--plugin-config must be JSON")?,
        None => serde_json::Value::Null,
    };
    println!("plugin    {}", o.argv.join(" "));

    let mut plugin = Plugin::new("check", o.argv.clone());
    let started = Instant::now();
    let manifest = plugin.init(config).await.context("initialize failed")?;
    let interval = match manifest.refresh {
        Refresh::Poll { interval_ms, .. } => interval_ms,
        Refresh::Push => 0,
    };
    println!(
        "manifest  {} {:?} · every {} · size {:?} · answered in {} ms",
        manifest.id,
        manifest.name,
        ms(interval),
        manifest.layout.size,
        started.elapsed().as_millis()
    );
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
    for i in 1..=o.renders.max(1) {
        if i > 1 {
            tokio::time::sleep(wait).await;
        }
        let started = Instant::now();
        let mut view = plugin
            .render()
            .await
            .with_context(|| format!("view/render {i} failed"))?;
        println!(
            "render {i}  {:?} · {:?} · {} ms",
            view.status,
            view.summary,
            started.elapsed().as_millis()
        );
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
    if o.json {
        println!("{}", serde_json::to_string_pretty(&view)?);
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
    let problems: Vec<&Finding> = findings.iter().filter(|f| f.problem).collect();
    for f in &problems {
        println!("problem   {}", f.text);
    }
    if problems.is_empty() {
        println!("no problems found");
        Ok(())
    } else {
        bail!("{} problem(s)", problems.len())
    }
}

fn ms(n: u64) -> String {
    if n >= 1000 && n.is_multiple_of(1000) {
        format!("{}s", n / 1000)
    } else {
        format!("{n}ms")
    }
}
