//! A fake Open-Meteo on 127.0.0.1 that answers with the recorded replies in
//! `crates/modules-personal/testdata/weather`. Shared by the weather tests here
//! and in `crates/host/tests` (included with `#[path]`).

use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Starts the server and returns its base URL. It runs until the test ends.
///
/// - `/v1/search`: Berlin for `name=Berlin`, no results for any other name.
/// - `/v1/forecast`: the imperial sample when asked for Fahrenheit, else the
///   metric one.
/// - any path with `/fail`: HTTP 400 with a `reason`, as Open-Meteo sends.
pub async fn start() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let req = String::from_utf8_lossy(&req);
                let target = req.split_whitespace().nth(1).unwrap_or("/");
                let (status, file) = answer(target);
                let body = std::fs::read(sample(file)).unwrap();
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&body).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    base
}

fn answer(target: &str) -> (&'static str, &'static str) {
    if target.contains("/fail") {
        ("400 Bad Request", "error.json")
    } else if target.starts_with("/v1/search") {
        if target.contains("name=Berlin&") {
            ("200 OK", "geocode-berlin.json")
        } else {
            ("200 OK", "geocode-none.json")
        }
    } else if target.contains("temperature_unit=fahrenheit") {
        ("200 OK", "forecast-imperial.json")
    } else {
        ("200 OK", "forecast.json")
    }
}

fn sample(file: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "../modules-personal/testdata/weather",
        file,
    ]
    .iter()
    .collect()
}

/// The config that points a weather tile at the fake server.
pub fn config(base: &str, mut extra: serde_json::Value) -> serde_json::Value {
    extra["forecast_url"] = format!("{base}/v1/forecast").into();
    extra["geocoding_url"] = format!("{base}/v1/search").into();
    extra
}
