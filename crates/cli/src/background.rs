//! `serve --detach`, `stop`, `status` and `open`: the web view as a background process.
//!
//! The server writes a small record (pid, URL, session token) to the runtime
//! dir once its port is bound; `stop`, `status` and `open` read it. The
//! detached server always writes it; a foreground `serve` writes it when no
//! other server has. One recorded server at a time: `serve --detach` while
//! one is running opens that one.
//!
//! The record holds the session token, so only its owner can read it. `open`
//! uses the token to ask the server for a new one-time sign-in link; the token
//! itself never goes in a URL.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;

pub struct Record {
    pub pid: u32,
    /// The page, without a token.
    pub url: String,
    /// The session token. `None` in a record from an older version.
    token: Option<String>,
}

fn record_path() -> Result<PathBuf> {
    Ok(dashboard_config::run_dirs()?.0.join("serve.json"))
}

fn log_path() -> Result<PathBuf> {
    Ok(dashboard_config::run_dirs()?.1.join("serve.log"))
}

/// Called by the server once it is listening.
pub fn write_record(url: &str, token: &str) -> Result<()> {
    let path = record_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let record = json!({ "pid": std::process::id(), "url": url, "token": token });
    // Written to a fresh 0600 file and renamed over the record, so the record
    // is never half-written and never keeps wider permissions from before.
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&tmp)
        .and_then(|mut file| file.write_all(record.to_string().as_bytes()))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.with_context(|| format!("writing {}", path.display()))
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
            token: value["token"].as_str().map(str::to_string),
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
/// `args` are the global options (`--config` and the like) to run it with.
pub fn detach(args: &[std::ffi::OsString], port: u16, dev_origin: Option<&str>) -> Result<Record> {
    let log = log_path()?;
    std::fs::create_dir_all(log.parent().unwrap())?;
    let out = OpenOptions::new().create(true).append(true).open(&log)?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(args);
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
        // A console of its own that is never shown, rather than none at all
        // (DETACHED_PROCESS): its plugins and commands share that console, so
        // each of them doesn't open a window of its own.
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
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
    // The server leads its own process group (see `detach`), so this also
    // ends its plugins and the commands they run. Only when the pid is still
    // a dashboard, not a reused pid that leads some other group. A plain kill
    // otherwise, or if that fails.
    #[cfg(unix)]
    let group = is_dashboard(record.pid)
        && Command::new("kill")
            .args(["-TERM", "--", &format!("-{}", record.pid)])
            .stderr(Stdio::null())
            .status()?
            .success();
    #[cfg(unix)]
    let status = match group {
        true => std::process::ExitStatus::default(),
        false => Command::new("kill").arg(record.pid.to_string()).status()?,
    };
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

/// A new one-time sign-in link for the recorded server: `POST /api/boot`
/// with the session token, over plain HTTP/1.1 to 127.0.0.1.
pub fn sign_in_link(record: &Record) -> Result<String> {
    let Some(token) = &record.token else {
        bail!(
            "the server at {} was started by an older version; stop it with `dashboard stop` and start it again",
            record.url
        );
    };
    let authority = record
        .url
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .with_context(|| format!("unexpected server URL {}", record.url))?;
    let mut stream = std::net::TcpStream::connect(authority)
        .with_context(|| format!("connecting to {}", record.url))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "POST /api/boot HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {token}\r\n\
         Content-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    if !head.starts_with("HTTP/1.1 200") {
        bail!(
            "the server at {} refused a new sign-in link ({})",
            record.url,
            head.lines().next().unwrap_or("no answer")
        );
    }
    let value: serde_json::Value =
        serde_json::from_str(body).context("reading the server's answer")?;
    let boot = value["boot"]
        .as_str()
        .context("the server's answer has no boot token")?;
    Ok(format!("{}?boot={boot}", record.url))
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
        .args(["/NH", "/FO", "CSV", "/FI", &format!("PID eq {pid}")])
        .output()
        .is_ok_and(|o| tasklist_has(&String::from_utf8_lossy(&o.stdout), pid));
    alive
}

/// Whether process `pid` runs this same program (by name, from `ps`).
#[cfg(unix)]
fn is_dashboard(pid: u32) -> bool {
    let Some(ours) = std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
    else {
        return false;
    };
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .is_ok_and(|o| same_program(&String::from_utf8_lossy(&o.stdout), &ours))
}

/// `comm` from `ps` names `ours`: a full path on macOS, the name cut to 15
/// characters on Linux.
#[cfg_attr(not(unix), allow(dead_code))]
fn same_program(comm: &str, ours: &str) -> bool {
    let comm = comm.trim();
    let name = Path::new(comm)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name == ours || (name.len() == 15 && ours.starts_with(&name))
}

/// Whether `tasklist /NH /FO CSV` output lists `pid`. Each process is a line
/// like `"dashboard.exe","4242","Console","1","12,345 K"`; with no match it
/// prints an "INFO: No tasks..." line instead.
#[cfg_attr(not(windows), allow(dead_code))]
fn tasklist_has(out: &str, pid: u32) -> bool {
    let pid = format!("\"{pid}\"");
    out.lines()
        .any(|line| line.split(',').nth(1).is_some_and(|field| field == pid))
}

/// Open `url` in the default browser. Skipped where there is no display.
pub fn open_browser(url: &str) {
    if let Err(err) = dashboard_host::browser::open(url) {
        crate::warn(&format!(
            "couldn't open a browser ({err}); open the link above within a minute"
        ))
    }
}

fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ps_comm() {
        assert!(same_program("/usr/local/bin/dashboard\n", "dashboard"));
        assert!(same_program("dashboard", "dashboard"));
        assert!(same_program("dashboard-relea", "dashboard-release"));
        assert!(!same_program("sshd", "dashboard"));
        assert!(!same_program("dash", "dashboard"));
        assert!(!same_program("", "dashboard"));
    }

    #[test]
    fn reads_tasklist_output() {
        let out = "\"dashboard.exe\",\"4242\",\"Console\",\"1\",\"12,345 K\"\r\n";
        assert!(tasklist_has(out, 4242));
        assert!(!tasklist_has(out, 42));
        assert!(!tasklist_has(out, 1));
        assert!(!tasklist_has(
            "INFO: No tasks are running which match the specified criteria.\r\n",
            4242
        ));
    }
}
