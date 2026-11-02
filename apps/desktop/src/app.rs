//! The Tauri side: windows for each mode, and the tray / menu-bar icon.
//!
//! Every window loads the page served by this process. Nothing in the page
//! talks to Tauri: it uses the same WebSocket as a browser, so the app needs no
//! IPC permissions.

use anyhow::Result;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Manager, Monitor, RunEvent, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::launch::{Launch, Mode};

/// The page URL, and the session token its windows sign in with.
struct Page(Url, String);

/// A navigation to this ends the screensaver (see `QUIT_ON_INPUT`).
const QUIT_SCHEME: &str = "dashboard-quit";

/// Run in screensaver windows: any key, click or real mouse move quits.
const QUIT_ON_INPUT: &str = r#"(() => {
  const quit = () => { location.href = 'dashboard-quit://now' }
  let start = null
  addEventListener('keydown', quit, true)
  addEventListener('mousedown', quit, true)
  addEventListener('mousemove', (e) => {
    // The first move event comes as the window appears; quit only on a real move.
    if (!start) { start = [e.screenX, e.screenY]; return }
    if (Math.abs(e.screenX - start[0]) + Math.abs(e.screenY - start[1]) > 10) quit()
  }, true)
})()"#;

/// `on_exit` runs as the app ends, before the process exits.
pub fn run(
    launch: Launch,
    url: Url,
    token: String,
    on_exit: impl Fn() + Send + 'static,
) -> Result<()> {
    let tray = launch.tray && matches!(launch.mode, Mode::Window { .. } | Mode::Wallpaper);
    let mode = launch.mode.clone();
    #[cfg(target_os = "macos")]
    let reopen = match launch.mode {
        Mode::Window { ambient } => Some(ambient),
        _ => None,
    };
    let app = tauri::Builder::default()
        .manage(Page(url, token))
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            if !matches!(mode, Mode::Window { .. }) {
                // No Dock icon for the wallpaper and the screensaver.
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            let handle = app.handle();
            match mode {
                Mode::Window { ambient } => {
                    open_window(handle, ambient)?;
                }
                Mode::Wallpaper => set_wallpaper(handle, true)?,
                Mode::Screensaver => screensaver(handle)?,
                Mode::Preview(parent) => preview(handle, parent)?,
                Mode::Configure => unreachable!("handled before the app starts"),
            }
            if tray {
                add_tray(handle, mode == Mode::Wallpaper)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())?;
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    app.run(move |app, event| match event {
        // With the tray icon, closing the last window keeps the app (and its
        // modules) running; Quit in the menu ends it.
        RunEvent::ExitRequested { api, code, .. } if tray && code.is_none() => {
            api.prevent_exit();
        }
        RunEvent::Exit => on_exit(),
        // macOS: a click on the Dock icon with no window open brings it back.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => {
            if let Some(ambient) = reopen
                && let Err(err) = open_window(app, ambient)
            {
                tracing::error!("{err}");
            }
        }
        _ => {}
    });
    Ok(())
}

fn page(app: &AppHandle, ambient: bool) -> Url {
    let mut url = app.state::<Page>().0.clone();
    if ambient {
        url.query_pairs_mut().append_pair("mode", "ambient");
    }
    url
}

/// Show the app window, or bring it to the front if it is open.
fn open_window(app: &AppHandle, ambient: bool) -> tauri::Result<WebviewWindow> {
    let label = if ambient { "ambient" } else { "main" };
    if let Some(w) = app.get_webview_window(label) {
        w.show()?;
        w.set_focus()?;
        return Ok(w);
    }
    let title = if ambient {
        "Dashboard (ambient)"
    } else {
        "Dashboard"
    };
    links(
        app,
        WebviewWindowBuilder::new(app, label, WebviewUrl::External(page(app, ambient))),
    )
    .title(title)
    .inner_size(1280.0, 800.0)
    .min_inner_size(480.0, 320.0)
    .build()
}

/// Sign the window in and keep it on the dashboard page: links to anything
/// else, and links that ask for a new window (`target="_blank"`,
/// `window.open`), open in the system browser instead.
fn links<'a>(
    app: &'a AppHandle,
    builder: WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let page = app.state::<Page>();
    let home = page.0.clone();
    builder
        .initialization_script(sign_in_script(&home, &page.1))
        .on_navigation(move |url| stays(&home, url))
        .on_new_window(|url, _| {
            open_external(&url);
            NewWindowResponse::Deny
        })
}

/// Put the session token where the page looks for one it kept earlier
/// (sessionStorage, see web/src/useDashboard.ts), so no window needs a
/// one-time boot link. Only on the dashboard's own origin.
fn sign_in_script(home: &Url, token: &str) -> String {
    let origin = home.origin().ascii_serialization();
    format!(
        "(() => {{ if (location.origin === {origin:?}) try {{ \
         sessionStorage.setItem('dashboard-session', {token:?}) }} catch {{}} }})()"
    )
}

/// Whether a navigation to `url` may load in the window: only pages of the
/// dashboard's own server. Anything else opens in the browser.
fn stays(home: &Url, url: &Url) -> bool {
    if url.origin() == home.origin() {
        return true;
    }
    open_external(url);
    false
}

/// Open an http(s) link in the system browser. Other schemes are dropped.
fn open_external(url: &Url) {
    if !matches!(url.scheme(), "http" | "https") {
        tracing::warn!("not opening a {} link", url.scheme());
        return;
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    // Not `cmd /c start`: cmd would treat `&` in the URL as a command separator.
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut cmd = std::process::Command::new("xdg-open");
    match cmd.arg(url.as_str()).spawn() {
        // Reaped, so each click doesn't leave a zombie in a long-running app.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(err) => tracing::error!("couldn't open the browser: {err}"),
    }
}

fn wallpaper_label(i: usize) -> String {
    format!("wallpaper-{i}")
}

/// Turn wallpaper mode on (one window per display) or off.
fn set_wallpaper(app: &AppHandle, on: bool) -> tauri::Result<()> {
    let open: Vec<_> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with("wallpaper-"))
        .map(|(_, w)| w)
        .collect();
    if !on {
        for w in open {
            w.close()?;
        }
        return Ok(());
    }
    if !open.is_empty() {
        return Ok(());
    }
    for (i, monitor) in app.available_monitors()?.iter().enumerate() {
        let window = covering(app, &wallpaper_label(i), monitor)?
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .always_on_bottom(true)
            .build()?;
        window.set_ignore_cursor_events(true)?;
        #[cfg(target_os = "macos")]
        crate::macos::pin_to_desktop(&window)?;
        #[cfg(windows)]
        crate::win::pin_to_desktop(&window, monitor)?;
    }
    Ok(())
}

/// One window per display, above everything, until input.
fn screensaver(app: &AppHandle) -> tauri::Result<()> {
    for (i, monitor) in app.available_monitors()?.iter().enumerate() {
        let quit = app.clone();
        let home = app.state::<Page>().0.clone();
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
        let window = covering(app, &format!("saver-{i}"), monitor)?
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(i == 0)
            .initialization_script(QUIT_ON_INPUT)
            // Replaces the guard from `links`, so it checks the page too.
            .on_navigation(move |url| {
                if url.scheme() == QUIT_SCHEME {
                    quit.exit(0);
                    return false;
                }
                stays(&home, url)
            })
            .build()?;
        #[cfg(target_os = "macos")]
        crate::macos::raise_to_screensaver(&window)?;
    }
    Ok(())
}

/// A borderless ambient window that covers `monitor`.
fn covering<'a>(
    app: &'a AppHandle,
    label: &str,
    monitor: &Monitor,
) -> tauri::Result<WebviewWindowBuilder<'a, tauri::Wry, AppHandle>> {
    let scale = monitor.scale_factor();
    let pos = monitor.position().to_logical::<f64>(scale);
    let size = monitor.size().to_logical::<f64>(scale);
    Ok(links(
        app,
        WebviewWindowBuilder::new(app, label, WebviewUrl::External(page(app, true))),
    )
    .title("Dashboard")
    .decorations(false)
    .resizable(false)
    .shadow(false)
    .position(pos.x, pos.y)
    .inner_size(size.width, size.height))
}

/// The screensaver inside the Control Panel's preview box (Windows `/p`).
fn preview(app: &AppHandle, parent: isize) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        let builder = links(
            app,
            WebviewWindowBuilder::new(app, "preview", WebviewUrl::External(page(app, true))),
        )
        .decorations(false)
        .skip_taskbar(true)
        .focused(false);
        crate::win::preview(app, builder, parent)?;
    }
    #[cfg(not(windows))]
    {
        // Only Windows has a preview box; elsewhere just show the screensaver.
        let _ = parent;
        screensaver(app)?;
    }
    Ok(())
}

fn add_tray(app: &AppHandle, wallpaper_on: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open window", true, None::<&str>)?;
    let ambient = MenuItem::with_id(app, "ambient", "Open ambient window", true, None::<&str>)?;
    let wallpaper = CheckMenuItem::with_id(
        app,
        "wallpaper",
        "Wallpaper",
        true,
        wallpaper_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &ambient, &wallpaper, &separator, &quit])?;
    let mut tray = TrayIconBuilder::with_id("dashboard")
        .tooltip("Dashboard")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            let result = match event.id().as_ref() {
                "open" => open_window(app, false).map(drop),
                "ambient" => open_window(app, true).map(drop),
                "wallpaper" => wallpaper.is_checked().and_then(|on| set_wallpaper(app, on)),
                "quit" => {
                    app.exit(0);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(err) = result {
                tracing::error!("{err}");
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_dashboard_page_stays_in_the_window() {
        let home: Url = "http://127.0.0.1:7878/".parse().unwrap();
        let same = "http://127.0.0.1:7878/?mode=ambient".parse().unwrap();
        assert!(stays(&home, &same));
        // Other schemes are dropped rather than opened, so the test opens nothing.
        let other = "file:///etc/passwd".parse().unwrap();
        assert!(!stays(&home, &other));
        let port = "ftp://127.0.0.1:7879/".parse().unwrap();
        assert!(!stays(&home, &port));
    }

    #[test]
    fn signs_in_on_the_dashboard_origin_only() {
        let home: Url = "http://127.0.0.1:7878/".parse().unwrap();
        let script = sign_in_script(&home, "abc");
        assert!(script.contains(r#"location.origin === "http://127.0.0.1:7878""#));
        assert!(script.contains(r#"setItem('dashboard-session', "abc")"#));
    }
}
