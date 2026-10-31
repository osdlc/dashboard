//! Shows the output of a command, run again on each refresh. Display only: it
//! runs the configured command and nothing else. Add it several times with
//! `builtin: cmdrunner`:
//!
//! ```yaml
//! modules:
//!   brew-outdated:
//!     builtin: cmdrunner
//!     refresh: 10m
//!     config:
//!       command: brew outdated   # run with `sh -c` (`cmd /C` on Windows)
//!       name: Outdated           # tile title; default the command
//!       lines: 40                # lines kept from the top (or the end, with tail)
//!       tail: false              # keep the last lines instead, e.g. for a log
//!       timeout_secs: 20
//! ```

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The command line, run with `sh -c` (`cmd /C` on Windows). Keep tokens
    /// in the environment: `$VARS` expand.
    command: Option<String>,
    /// Tile title. Default: the command.
    name: Option<String>,
    /// Lines shown.
    lines: usize,
    /// Show the last lines instead of the first.
    tail: bool,
    /// Seconds the command may run before it is stopped.
    timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command: None,
            name: None,
            lines: 40,
            tail: false,
            timeout_secs: 20,
        }
    }
}

#[derive(Default)]
pub struct CmdRunner {
    config: Config,
    command: String,
    name: String,
}

#[async_trait]
impl Module for CmdRunner {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let command = self.config.command.clone().unwrap_or_default();
        if command.trim().is_empty() {
            bail!("the cmdrunner module needs `config: {{ command: <command line> }}`");
        }
        if self.config.lines == 0 {
            bail!("`lines` must be at least 1");
        }
        if self.config.timeout_secs == 0 {
            bail!("`timeout_secs` must be at least 1");
        }
        self.name = self.config.name.clone().unwrap_or_else(|| command.clone());
        self.command = command;
        Ok(manifest(
            "cmdrunner",
            &self.name,
            60_000,
            1_000,
            Size::Medium,
            0,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let mut cmd = shell(&self.command);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Its own process group, so a timeout stops the whole pipeline, not
        // only `sh`.
        #[cfg(unix)]
        cmd.process_group(0);
        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("running the command: {e}"))?;
        let _group = Group(child.id());
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let run = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => Run {
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Ok(Err(e)) => bail!("running the command: {e}"),
            Err(_) => bail!("the command ran longer than {}s", self.config.timeout_secs),
        };
        Ok(view(&run, self.config.lines, self.config.tail))
    }
}

/// The command's process group. Dropping it kills what is left of the group:
/// after a timeout, or children the command left behind.
#[cfg_attr(not(unix), allow(dead_code))]
struct Group(Option<u32>);

impl Drop for Group {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.and_then(|p| libc::pid_t::try_from(p).ok()) {
            // SAFETY: killpg takes plain integers. The group id is the pid of
            // the child we spawned as a group leader; ESRCH when it is gone.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
}

fn shell(line: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(line);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(line);
        cmd
    }
}

/// What one run of the command gave.
struct Run {
    /// None when a signal stopped it.
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The tile: the output as text. A failed command is an error tile with its
/// output (stdout, else stderr), so the reason shows.
fn view(run: &Run, max_lines: usize, tail: bool) -> View {
    let ok = run.code == Some(0);
    let out = if ok || !run.stdout.trim().is_empty() {
        &run.stdout
    } else {
        &run.stderr
    };
    let text = strip_ansi(out).replace("\r\n", "\n");
    let text = text.trim_end();
    let lines: Vec<&str> = if text.is_empty() {
        vec![]
    } else {
        text.split('\n').collect()
    };
    let shown = if lines.is_empty() {
        "(no output)".to_string()
    } else if lines.len() <= max_lines {
        lines.join("\n")
    } else if tail {
        format!("…\n{}", lines[lines.len() - max_lines..].join("\n"))
    } else {
        format!("{}\n…", lines[..max_lines].join("\n"))
    };
    let count = match lines.len() {
        1 => "1 line".to_string(),
        n => format!("{n} lines"),
    };
    let (status, summary) = match run.code {
        Some(0) => (Status::Ok, count),
        Some(code) => (Status::Error, format!("exit {code} · {count}")),
        None => (Status::Error, "stopped by a signal".to_string()),
    };
    View {
        status,
        summary,
        root: Widget::Text {
            text: shown,
            markdown: false,
        },
    }
}

/// Drops terminal color and cursor codes (ESC [ ... letter), which many
/// commands write even when not on a terminal.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The demo: `df -h` output, as cmdrunner shows it.
const DEMO: &str = include_str!("../testdata/cmdrunner/df.txt");

pub fn catalog() -> CatalogEntry {
    let run = Run {
        code: Some(0),
        stdout: DEMO.into(),
        stderr: String::new(),
    };
    CatalogEntry {
        description: "Shows the output of a command, run again on each refresh. Display \
                      only. Add it several times with `builtin: cmdrunner`."
            .into(),
        tags: crate::strings(&["command", "local", "shell"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("cmdrunner", "Command", Size::Medium, view(&run, 40, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(v: &View) -> &str {
        match &v.root {
            Widget::Text { text, .. } => text,
            other => panic!("text: {other:?}"),
        }
    }

    fn run(code: Option<i32>, stdout: &str, stderr: &str) -> Run {
        Run {
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn shows_the_sample_output() {
        let v = view(&run(Some(0), DEMO, ""), 40, false);
        assert_eq!(v.status, Status::Ok);
        assert_eq!(v.summary, "5 lines");
        assert!(text(&v).starts_with("Filesystem"));
        assert!(!text(&v).ends_with('\n'));
    }

    #[test]
    fn head_tail_and_failures() {
        let out = "a\nb\nc\nd\n";
        assert_eq!(text(&view(&run(Some(0), out, ""), 2, false)), "a\nb\n…");
        assert_eq!(text(&view(&run(Some(0), out, ""), 2, true)), "…\nc\nd");

        let v = view(&run(Some(2), "", "ls: nope: No such file\n"), 10, false);
        assert_eq!(v.status, Status::Error);
        assert_eq!(v.summary, "exit 2 · 1 line");
        assert_eq!(text(&v), "ls: nope: No such file");

        let v = view(&run(Some(0), "", ""), 10, false);
        assert_eq!(text(&v), "(no output)");
        assert_eq!(
            view(&run(None, "", ""), 10, false).summary,
            "stopped by a signal"
        );
    }

    #[test]
    fn strips_color_codes() {
        assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_the_command() {
        let mut m = CmdRunner::default();
        let manifest = m
            .init(serde_json::json!({ "command": "printf 'one\\ntwo\\n'; exit 3" }))
            .await
            .unwrap();
        assert_eq!(manifest.name, "printf 'one\\ntwo\\n'; exit 3");
        let v = m.render().await.unwrap();
        assert_eq!(v.status, Status::Error);
        assert_eq!(text(&v), "one\ntwo");

        let mut m = CmdRunner::default();
        m.init(serde_json::json!({ "command": "sleep 5", "timeout_secs": 1 }))
            .await
            .unwrap();
        assert!(m.render().await.is_err());
    }

    /// A timeout stops the whole pipeline, not only `sh`.
    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_stops_the_pipeline() {
        let dir = std::env::temp_dir().join(format!("cmdrunner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("pid");
        let command = format!("sleep 30 & echo $! > {}; wait", pidfile.display());
        let mut m = CmdRunner::default();
        m.init(serde_json::json!({ "command": command, "timeout_secs": 1 }))
            .await
            .unwrap();
        assert!(m.render().await.is_err());
        let pid = std::fs::read_to_string(&pidfile).unwrap();
        let pid: libc::pid_t = pid.trim().parse().unwrap();
        std::fs::remove_dir_all(&dir).ok();
        // Give the kill a moment, then the sleep must be gone.
        tokio::time::sleep(Duration::from_millis(200)).await;
        // SAFETY: signal 0 only checks that the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "sleep {pid} still running");
    }

    #[tokio::test]
    async fn config_errors() {
        for config in [
            serde_json::json!({}),
            serde_json::json!({ "command": " " }),
            serde_json::json!({ "command": "ls", "lines": 0 }),
            serde_json::json!({ "command": "ls", "cmd": "x" }),
        ] {
            assert!(CmdRunner::default().init(config).await.is_err());
        }
    }

    #[test]
    fn demo_is_valid() {
        let problems: Vec<String> = dashboard_contract::check::check_view(&catalog().demo)
            .into_iter()
            .filter(|f| f.problem)
            .map(|f| f.text)
            .collect();
        assert!(problems.is_empty(), "{problems:?}");
    }
}
