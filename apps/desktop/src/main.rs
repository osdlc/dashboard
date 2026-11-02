//! dashboard-desktop: the dashboard in its own window, as the desktop
//! wallpaper, or as a screensaver. It runs the same modules and web server as
//! `dashboard serve` (on 127.0.0.1, any free port) and loads that page in the
//! system webview. See docs/desktop.md.

// Release builds on Windows get no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use dashboard_config::layers::Sources;
use launch::{Launch, Mode};

mod app;
mod launch;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Bundled plugins run as `<exe> plugin <id>` (see dashboard-runtime).
    if let [cmd, id] = args.as_slice()
        && cmd == "plugin"
        && let Some(result) = dashboard_runtime::run_bundled(id)
    {
        return result;
    }

    let scr = std::env::current_exe()
        .ok()
        .and_then(|p| p.extension().map(|e| e.eq_ignore_ascii_case("scr")))
        .unwrap_or(false);
    let launch = match launch::parse(&args, scr) {
        Ok(l) => l,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    if launch.mode == Mode::Configure {
        return configure(launch.config.as_deref());
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let (url, token, host) = tauri::async_runtime::block_on(serve(&launch))?;
    // The app ends the process without dropping the async runtime, so the
    // modules' tasks are never dropped: stop them, and their plugins, first.
    let stop = move || tauri::async_runtime::block_on(host.stop_all());
    app::run(launch, url, token, stop)
}

/// Start the modules and the web server. Returns the page URL and the
/// session token the app's windows sign in with, and the host running the
/// modules.
async fn serve(launch: &Launch) -> Result<(tauri::Url, String, dashboard_runtime::Host)> {
    // The machine file and a saved dashboard; no project file, since the app
    // has no working folder of its own.
    let sources = Sources {
        machine: launch.config.clone(),
        ..Default::default()
    };
    let (host, settings, actions, layer_paths) = dashboard_runtime::start(sources).await?;
    let (ready, url) = tokio::sync::oneshot::channel::<(String, String)>();
    let config = dashboard_web::WebConfig {
        port: launch.port,
        dev_origin: None,
        on_ready: Some(Box::new(move |r: &dashboard_web::Ready| {
            let _ = ready.send((r.url.clone(), r.token.clone()));
        })),
        catalog: Some(Arc::new(dashboard_runtime::catalog_source(layer_paths))),
    };
    let ours = host.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = dashboard_web::serve(host, settings, actions, config).await {
            tracing::error!("web server stopped: {err:#}");
        }
    });
    let (url, token) = url.await.context("the web server didn't start")?;
    Ok((url.parse()?, token, ours))
}

/// The screensaver's settings: it has none of its own, so say which config
/// file it shows.
fn configure(config: Option<&Path>) -> Result<()> {
    let path = match config {
        Some(p) => p.to_path_buf(),
        None => dashboard_config::default_path()?,
    };
    let message = format!(
        "The dashboard screensaver shows the screens from\n{}\n\nEdit that file, or use \
         the editor in `dashboard serve`, to change them.",
        path.display()
    );
    #[cfg(windows)]
    win::message_box("Dashboard screensaver", &message);
    #[cfg(not(windows))]
    println!("{message}");
    Ok(())
}
