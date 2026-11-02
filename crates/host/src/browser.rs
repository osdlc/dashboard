//! Opening a link in the default browser. Shared by `dashboard serve` (which
//! opens its own page) and the TUI (which opens links in tiles).

use std::process::{Command, Stdio};

/// Open `url` in the default browser. Returns `Ok(false)` where there is no
/// display to open it on (a Linux box without X or Wayland).
pub fn open(url: &str) -> std::io::Result<bool> {
    #[cfg(target_os = "macos")]
    let cmd = Some(("open", vec![url]));
    // Not `cmd /C start`: cmd would treat `&` in the URL as a new command.
    #[cfg(windows)]
    let cmd = Some(("rundll32", vec!["url.dll,FileProtocolHandler", url]));
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = (std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some())
    .then(|| ("xdg-open", vec![url]));
    let Some((program, args)) = cmd else {
        return Ok(false);
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| std::io::Error::new(e.kind(), format!("{program}: {e}")))?;
    // Reap it, so a long-running process doesn't keep a zombie.
    drop(std::thread::spawn(move || child.wait()));
    Ok(true)
}

/// Like `open`, but only for http(s) links, so a module can't make this run
/// something else (a `file:` URL, an app's custom scheme).
pub fn open_web_link(url: &str) -> std::io::Result<bool> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Ok(false);
    }
    open(url)
}
