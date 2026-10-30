//! Runs a stdio plugin as a module: JSON-RPC 2.0, one message per line.
//!
//! Poll mode only for now. If the process dies or a request times out, the
//! process is dropped (and killed) and started again on the next refresh, so
//! the poll period doubles as the restart backoff.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use dashboard_contract::rpc::{
    HostInfo, InitializeParams, InitializeResult, LogLevel, LogParams, Message, Request, method,
};
use dashboard_contract::{CONTRACT_VERSION, Manifest, Refresh, View};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::Module;

const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Plugin {
    tile_id: String,
    argv: Vec<String>,
    dir: Option<std::path::PathBuf>,
    config: serde_json::Value,
    proc: Option<Proc>,
    next_id: u64,
}

struct Proc {
    // Held so the process is killed when this is dropped.
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
}

impl Plugin {
    pub fn new(tile_id: &str, argv: Vec<String>) -> Self {
        Self {
            tile_id: tile_id.into(),
            argv,
            dir: None,
            config: serde_json::Value::Null,
            proc: None,
            next_id: 1,
        }
    }

    /// Run the plugin in this folder, so relative paths in its arguments
    /// (and its own file reads) resolve there.
    pub fn in_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Start the process and run the handshake.
    async fn start_process(&mut self) -> Result<Manifest> {
        let (program, args) = self
            .argv
            .split_first()
            .context("plugin needs a program name")?;
        let mut command = Command::new(program);
        if let Some(dir) = &self.dir {
            command.current_dir(dir);
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting plugin {program:?}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
        // stderr goes to the log, so it can't corrupt the TUI.
        let mut stderr = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
        let tile = self.tile_id.clone();
        tokio::spawn(async move {
            while let Ok(Some(line)) = stderr.next_line().await {
                tracing::info!(tile = %tile, "plugin stderr: {line}");
            }
        });
        self.proc = Some(Proc {
            _child: child,
            stdin,
            stdout,
        });

        let params = InitializeParams {
            contract_version: CONTRACT_VERSION.into(),
            host: HostInfo {
                name: "dashboard".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            config: self.config.clone(),
        };
        let result: InitializeResult = self
            .call(method::INITIALIZE, Some(serde_json::to_value(params)?))
            .await?;
        if result.manifest.refresh == Refresh::Push {
            bail!("push-mode plugins are not supported yet");
        }
        Ok(result.manifest)
    }

    /// Send a request and wait for its response, with a timeout. Any failure
    /// drops the process so the next call starts a fresh one.
    async fn call<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<T> {
        let result = tokio::time::timeout(TIMEOUT, self.exchange(method, params))
            .await
            .unwrap_or_else(|_| Err(anyhow!("plugin did not answer {method} within {TIMEOUT:?}")));
        if result.is_err() {
            self.proc = None;
        }
        result
    }

    async fn exchange<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<T> {
        let id = self.next_id;
        self.next_id += 1;
        let proc = self.proc.as_mut().context("plugin is not running")?;
        let req = Request {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        };
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        proc.stdin
            .write_all(line.as_bytes())
            .await
            .context("writing to plugin")?;
        proc.stdin.flush().await?;

        loop {
            let line = proc.stdout.next_line().await?.context("plugin exited")?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Message>(&line) {
                Ok(Message::Response(resp)) if resp.id == id => {
                    if let Some(err) = resp.error {
                        bail!("{} (code {})", err.message, err.code);
                    }
                    let value = resp.result.unwrap_or(serde_json::Value::Null);
                    return serde_json::from_value(value)
                        .with_context(|| format!("bad {method} result"));
                }
                Ok(Message::Notification(n)) if n.method == method::LOG => {
                    if let Some(p) = n
                        .params
                        .and_then(|p| serde_json::from_value::<LogParams>(p).ok())
                    {
                        let tile = &self.tile_id;
                        match p.level {
                            LogLevel::Error => tracing::error!(tile = %tile, "{}", p.message),
                            LogLevel::Warn => tracing::warn!(tile = %tile, "{}", p.message),
                            LogLevel::Info => tracing::info!(tile = %tile, "{}", p.message),
                            LogLevel::Debug => tracing::debug!(tile = %tile, "{}", p.message),
                        }
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(tile = %self.tile_id, "ignoring bad plugin output: {err}")
                }
            }
        }
    }
}

#[async_trait]
impl Module for Plugin {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        self.config = config;
        self.start_process().await
    }

    async fn render(&mut self) -> Result<View> {
        if self.proc.is_none() {
            self.start_process().await.context("restarting plugin")?;
        }
        self.call(method::VIEW_RENDER, None).await
    }
}
