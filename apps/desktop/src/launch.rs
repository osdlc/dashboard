//! What the app was started to do, from its command line.
//!
//! Besides its own flags it takes the Windows screensaver arguments: Windows
//! runs a `.scr` with `/s` (show), `/p <hwnd>` (preview inside the Control
//! Panel's little window) or `/c` (settings), and with no arguments when the
//! user picks "Settings" from Explorer's context menu. They are accepted on
//! every OS, so they can be tried anywhere.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// A normal app window. `ambient` loads the page with `?mode=ambient`.
    Window { ambient: bool },
    /// One borderless ambient window per display, behind the desktop icons,
    /// that ignores clicks.
    Wallpaper,
    /// One ambient window per display, above everything, until a key, click
    /// or mouse move.
    Screensaver,
    /// The screensaver drawn inside another window (Windows `/p <hwnd>`).
    Preview(isize),
    /// The screensaver's settings (Windows `/c`).
    Configure,
}

#[derive(Debug, PartialEq)]
pub struct Launch {
    pub mode: Mode,
    pub config: Option<PathBuf>,
    /// 0 picks a free port.
    pub port: u16,
    /// Show the tray / menu-bar icon (window and wallpaper modes).
    pub tray: bool,
}

pub const USAGE: &str = "\
usage: dashboard-desktop [--config FILE] [--port N] [--ambient | --wallpaper | --screensaver] [--no-tray]

  --config FILE   config file (default: the same one `dashboard` uses)
  --port N        serve on this port (default: any free port)
  --ambient       open the window in ambient mode: no controls, screens rotating
  --wallpaper     one ambient window per display, on the desktop behind the icons
  --screensaver   one full-screen ambient window per display until a key or mouse move
  --no-tray       no tray / menu-bar icon; quit when the last window closes
                  (not with --wallpaper: nothing could quit it)

Windows screensaver arguments (when the exe is renamed to .scr): /s, /p <hwnd>, /c";

/// Parse the arguments after the program name. `scr` is true when the
/// executable's name ends in `.scr`, where no arguments means "settings".
pub fn parse(args: &[String], scr: bool) -> Result<Launch, String> {
    let mut launch = Launch {
        mode: Mode::Window { ambient: false },
        config: None,
        port: 0,
        tray: true,
    };
    if scr && args.is_empty() {
        launch.mode = Mode::Configure;
        return Ok(launch);
    }
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        // Windows passes `/p 1234`, `/p:1234` or `-p 1234`, in any case.
        let lower = arg.to_ascii_lowercase();
        let (flag, attached) = match lower.split_once(':') {
            Some((f, v)) if f.starts_with('/') => (f.to_string(), Some(v.to_string())),
            _ => (lower.clone(), None),
        };
        match flag.as_str() {
            "/s" | "-s" => launch.mode = Mode::Screensaver,
            "/c" | "-c" => launch.mode = Mode::Configure,
            "/p" | "-p" => {
                let value = attached.or_else(|| args.next().cloned());
                let hwnd = value
                    .and_then(|v| v.parse::<isize>().ok())
                    .ok_or("/p needs a window handle")?;
                launch.mode = Mode::Preview(hwnd);
            }
            "--ambient" => launch.mode = Mode::Window { ambient: true },
            "--wallpaper" => launch.mode = Mode::Wallpaper,
            "--screensaver" => launch.mode = Mode::Screensaver,
            "--no-tray" => launch.tray = false,
            "--config" => {
                let path = args.next().ok_or("--config needs a file")?;
                launch.config = Some(PathBuf::from(path));
            }
            "--port" => {
                launch.port = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--port needs a number")?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            _ => return Err(format!("unknown argument {arg:?}\n\n{USAGE}")),
        }
    }
    // Wallpaper windows ignore clicks, so the tray's Quit is the only way out.
    if launch.mode == Mode::Wallpaper && !launch.tray {
        return Err(format!(
            "--wallpaper needs the tray icon to quit; drop --no-tray\n\n{USAGE}"
        ));
    }
    Ok(launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(args: &[&str], scr: bool) -> Mode {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&args, scr).unwrap().mode
    }

    #[test]
    fn windows_screensaver_arguments() {
        assert_eq!(mode(&["/s"], true), Mode::Screensaver);
        assert_eq!(mode(&["/S"], true), Mode::Screensaver);
        assert_eq!(mode(&["/p", "1234"], true), Mode::Preview(1234));
        assert_eq!(mode(&["/P:5678"], true), Mode::Preview(5678));
        assert_eq!(mode(&["/c"], true), Mode::Configure);
        assert_eq!(mode(&["/c:42"], true), Mode::Configure);
        // Explorer's "Settings" runs the .scr with no arguments.
        assert_eq!(mode(&[], true), Mode::Configure);
        assert!(parse(&["/p".into()], true).is_err());
    }

    #[test]
    fn own_flags() {
        assert_eq!(mode(&[], false), Mode::Window { ambient: false });
        assert_eq!(mode(&["--ambient"], false), Mode::Window { ambient: true });
        assert_eq!(mode(&["--wallpaper"], false), Mode::Wallpaper);
        let args: Vec<String> = ["--config", "a.yaml", "--port", "9000", "--no-tray"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let launch = parse(&args, false).unwrap();
        assert_eq!(launch.config, Some(PathBuf::from("a.yaml")));
        assert_eq!(launch.port, 9000);
        assert!(!launch.tray);
        assert!(parse(&["--nope".into()], false).is_err());
        assert!(parse(&["--wallpaper".into(), "--no-tray".into()], false).is_err());
    }
}
