//! Stands in for dashboard-host in the WASI build: the parts the clock and
//! textfile modules use, without tokio.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use async_trait::async_trait;
use dashboard_contract::{Manifest, View};

/// The same trait as dashboard_host::Module, poll mode only.
#[async_trait]
pub trait Module: Send + 'static {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest>;
    async fn render(&mut self) -> Result<View>;
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
