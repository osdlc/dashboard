//! `serve --detach`, `stop` and `status`: the web view as a background process.
//!
//! The detached server writes a small record (pid, port, URL) to the runtime
//! dir once its port is bound; `stop` and `status` read it. One background
//! server at a time: `serve --detach` while one is running opens that one.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;

pub struct Record {
    pub pid: u32,
    pub url: String,
}

fn record_path() -> Result<PathBuf> {
    Ok(dashboard_config::run_dirs()?.0.join("serve.json"))
}

fn log_path() -> Result<PathBuf> {
    Ok(dashboard_config::run_dirs()?.1.join("serve.log"))
}

/// Called by the detached server once it is listening.
pub fn write_record(url: &str) -> Result<()> {
    let path = record_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let record = json!({ "pid": std::process::id(), "url": url });
    std::fs::write(&path, record.to_string()).with_context(|| format!("writing {}", path.display()))
}

/// The running background server, if any. A record whose process is gone is removed.
pub fn running() -> Result<Option<Record>> {
    let path = record_path()?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let record = value["pid"]
        .as_u64()
        .zip(value["url"].as_str())
        .map(|(pid, url)| Record {
            pid: pid as u32,
            url: url.to_string(),
        });
    match record {
        Some(r) if alive(r.pid) => Ok(Some(r)),
        _ => {
            let _ = std::fs::remove_file(&path);
            Ok(None)
        }
    }
}

/// Start `dashboard serve` again as a detached process (its own process group,
/// output to the log file) and wait until it is listening.
pub fn detach(config: Option<&Path>, port: u16, dev_origin: Option<&str>) -> Result<Record> {
    let log = log_path()?;
    std::fs::create_dir_all(log.parent().unwrap())?;
    let out = OpenOptions::new().create(true).append(true).open(&log)?;
    let mut cmd = Command::new(std::env::current_exe()?);
    if let Some(config) = config {
        cmd.arg("--config").arg(std::path::absolute(config)?);
    }
    cmd.args([
        "serve",
        "--no-open",
        "--record",
        "--port",
        &port.to_string(),
    ]);
    if let Some(origin) = dev_origin {
        cmd.args(["--dev-origin", origin]);
    }
    cmd.stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so Ctrl-C or closing the terminal doesn't reach it.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x8;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = cmd.spawn().context("starting the background server")?;

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if let Some(status) = child.try_wait()? {
            bail!(
                "the background server exited ({status}). Its log, {}:\n{}",
                log.display(),
                tail(&log, 15)
            );
        }
        if let Some(record) = running()?
            && record.pid == child.id()
        {
            return Ok(record);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!(
        "the background server didn't start in 20s; see {}",
        log.display()
    )
}

pub fn stop() -> Result<Option<Record>> {
    let Some(record) = running()? else {
        return Ok(None);
    };
    #[cfg(unix)]
    let status = Command::new("kill").arg(record.pid.to_string()).status()?;
    // /T also ends its plugin processes.
    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/PID", &record.pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("couldn't stop process {}", record.pid);
    }
    let _ = std::fs::remove_file(record_path()?);
    Ok(Some(record))
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    let alive = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    #[cfg(windows)]
    let alive = Command::new("tasklist")
        .args(["/NH", "/FI", &format!("PID eq {pid}")])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()));
    alive
}

/// Open `url` in the default browser. Skipped where there is no display.
pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = Some(("open", vec![url]));
    #[cfg(windows)]
    let cmd = Some(("rundll32", vec!["url.dll,FileProtocolHandler", url]));
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = (std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some())
    .then(|| ("xdg-open", vec![url]));
    let Some((program, args)) = cmd else { return };
    let spawned = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        // Reap it, so a long-running server doesn't keep a zombie.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(err) => {
            eprintln!("couldn't open a browser ({program}: {err}); open the URL above yourself")
        }
    }
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}
