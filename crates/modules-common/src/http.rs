//! HTTP helpers for modules that call a web API. Behind the `http` feature, so
//! only the bundles that need reqwest build it.

use std::time::Duration;

use anyhow::{Result, bail};
/// So modules can name these types without depending on reqwest themselves.
pub use reqwest::{Client, Response};

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("dashboard/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// A client for a server on this machine (127.0.0.1) that uses a self-signed
/// certificate, such as a desktop app's local language server. Never use it
/// for a remote host: it accepts any certificate.
pub fn localhost_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("dashboard/", env!("CARGO_PKG_VERSION")))
        .danger_accept_invalid_certs(true)
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
