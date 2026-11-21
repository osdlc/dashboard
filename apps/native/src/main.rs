//! sitroom-native: sitroom drawn with GPUI, in a window, as the desktop
//! wallpaper, or as a screensaver. It runs the same modules as `sitroom serve`
//! and reads the host directly, as the TUI does, with no web server or webview.

// Release builds on Windows get no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use anyhow::Result;
use gpui::App;
use launch::{Launch, Mode};
use sitroom_config::layers::Sources;

mod app;
mod catalog;
mod desktop;
mod editor;
mod grid;
mod input;
// The same command line as the Tauri app. `--port` has no use here.
#[allow(dead_code)]
#[path = "../../desktop/src/launch.rs"]
mod launch;
mod motion;
mod nav;
mod panels;
mod theme;
mod widgets;

use app::Shared;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Bundled plugins run as `<exe> plugin <id>` (see sitroom-runtime).
    if let [cmd, id] = args.as_slice()
        && cmd == "plugin"
        && let Some(result) = sitroom_runtime::run_bundled(id)
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
        let path = match &launch.config {
            Some(path) => path.clone(),
            None => sitroom_config::default_path()?,
        };
        println!(
            "Sitroom screensaver shows the screens from\n{}",
            path.display()
        );
        return Ok(());
    }

    let runtime = tokio::runtime::Runtime::new()?;
    // The machine file and a saved dashboard; no project file, since the app
    // has no working folder of its own.
    let sources = Sources {
        machine: launch.config.clone(),
        ..Default::default()
    };
    let (host, settings, actions, layers) = runtime.block_on(sitroom_runtime::start(sources))?;
    let shared = Shared {
        host,
        settings,
        actions,
        catalog: Arc::new(sitroom_runtime::catalog_source(layers)),
        runtime: runtime.handle().clone(),
    };
    run(launch, shared);
    Ok(())
}

fn run(launch: Launch, shared: Shared) {
    let tray = launch.tray
        && cfg!(any(target_os = "macos", windows))
        && matches!(launch.mode, Mode::Window { .. } | Mode::Wallpaper);
    let reopen = shared.clone();
    let reopen_ambient = matches!(launch.mode, Mode::Window { ambient: true });
    let can_reopen = matches!(launch.mode, Mode::Window { .. });
    let app = gpui_platform::application();
    app.on_reopen(move |cx| {
        // macOS: a click on the Dock icon with no window open brings it back.
        if can_reopen && cx.windows().is_empty() {
            desktop::open_window(&reopen, reopen_ambient, cx);
        }
    });
    app.run(move |cx: &mut App| {
        let stop = shared.host.clone();
        let handle = shared.runtime.clone();
        // The app ends the process without dropping the runtime, so the
        // modules' tasks are never dropped: stop them, and their plugins, first.
        cx.on_app_quit(move |_| {
            let (stop, handle) = (stop.clone(), handle.clone());
            async move {
                let _ = handle.spawn(async move { stop.stop_all().await }).await;
            }
        })
        .detach();
        // With the tray icon, closing the last window keeps the app (and its
        // modules) running; Quit in the menu ends it.
        if !tray {
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        }
        if !matches!(launch.mode, Mode::Window { .. }) {
            // No Dock icon for the wallpaper and the screensaver.
            cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
        }
        let wallpaper = match launch.mode {
            Mode::Window { ambient } => {
                desktop::open_window(&shared, ambient, cx);
                vec![]
            }
            Mode::Wallpaper => desktop::wallpaper(&shared, cx),
            Mode::Screensaver => {
                desktop::screensaver(&shared, cx);
                vec![]
            }
            Mode::Preview(parent) => {
                desktop::preview(&shared, parent, cx);
                vec![]
            }
            Mode::Configure => unreachable!("handled before the app starts"),
        };
        #[cfg(any(target_os = "macos", windows))]
        if tray {
            desktop::tray::add(shared.clone(), wallpaper, cx);
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = wallpaper;
    });
}
