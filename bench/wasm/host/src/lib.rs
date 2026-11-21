//! Stands in for sitroom-core's `host` module in the WASI build: the parts the clock and
//! textfile modules use, without tokio.

/// `sitroom_core::host`, as far as the modules need it.
pub mod host {
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::Result;
    use async_trait::async_trait;
    use sitroom_contract::{Manifest, View};

    /// The same trait as sitroom_core::host::Module, poll mode only.
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
}
