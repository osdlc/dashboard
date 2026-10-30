//! HTTP helpers for modules that call a web API. Behind the `http` feature, so
//! only the bundles that need reqwest build it.

use std::time::Duration;

use anyhow::{Result, bail};

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("dashboard/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Turns a non-2xx response into an error with the start of its body.
pub async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let body: String = body.chars().take(300).collect();
    let hint = match status.as_u16() {
        401 | 403 => " (check the token and its permissions)",
        _ => "",
    };
    bail!("HTTP {status}{hint}: {body}")
}
