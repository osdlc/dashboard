//! Shows what a URL returns, fetched again on each refresh: a view or widget
//! in the contract's JSON, values picked out of any JSON, or text. Add it
//! several times with `builtin: http`:
//!
//! ```yaml
//! modules:
//!   deploys:
//!     builtin: http
//!     refresh: 30s
//!     config:
//!       url: https://ci.example.com/dashboard/view.json
//!       token_env: CI_TOKEN            # sent as `Authorization: Bearer ...`
//!   queue:
//!     builtin: http
//!     config:
//!       url: http://localhost:9000/stats
//!       name: Queue
//!       fields:                        # pick values out of any JSON
//!         - { label: Waiting, pointer: /queue/waiting }
//!         - { label: p99, pointer: /latency/p99, unit: ms }
//! ```

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Size, View};
use dashboard_host::Module;
use dashboard_modules_common::http::{self, Client};
use dashboard_modules_common::manifest;
use dashboard_modules_common::secret::Secret;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::feed::{self, Field, Format, MAX_BYTES, Shape};

/// Headers that carry secrets; those come from `token_env` or `token_command`.
const SECRET_HEADERS: &[&str] = &["authorization", "cookie", "proxy-authorization"];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The http:// or https:// URL to fetch.
    url: Option<String>,
    /// Tile title. Default: the URL's host.
    name: Option<String>,
    /// How to read the response.
    format: Format,
    /// Values to pick out of a JSON response, each shown as a stat.
    fields: Vec<Field>,
    /// Render text as markdown. Default: true when the response says
    /// `text/markdown`.
    markdown: Option<bool>,
    /// Lines of text shown, from the top.
    lines: usize,
    /// Other request headers, e.g. `Accept`. No secrets here: use
    /// `token_env` or `token_command`.
    headers: BTreeMap<String, String>,
    /// The header the token goes in. Default `Authorization`, sent as
    /// `Bearer <token>`; any other header gets the token as it is.
    token_header: Option<String>,
    #[serde(flatten)]
    secret: Secret,
    /// Seconds a request may take.
    timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: None,
            name: None,
            format: Format::Auto,
            fields: vec![],
            markdown: None,
            lines: 200,
            headers: BTreeMap::new(),
            token_header: None,
            secret: Secret::default(),
            timeout_secs: 20,
        }
    }
}

#[derive(Default)]
pub struct Http {
    config: Config,
    url: String,
    name: String,
    client: Option<Client>,
    /// The last response's ETag and the view built from it: a 304 reuses it.
    cached: Option<(String, View)>,
}

#[async_trait]
impl Module for Http {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let Some(url) = self.config.url.clone() else {
            bail!("the http module needs `config: {{ url: <url> }}`");
        };
        let Some(host) = host(&url) else {
            bail!("`url` must start with http:// or https://, got {url:?}");
        };
        feed::check(self.config.format, &self.config.fields, self.config.lines)?;
        if self.config.timeout_secs == 0 {
            bail!("`timeout_secs` must be at least 1");
        }
        for key in self.config.headers.keys() {
            if SECRET_HEADERS.contains(&key.to_ascii_lowercase().as_str()) {
                bail!("put the {key} value in an environment variable and name it with token_env");
            }
        }
        if self.config.token_header.is_some()
            && self.config.secret.token_env.is_none()
            && self.config.secret.token_command.is_none()
        {
            bail!("`token_header` needs `token_env` or `token_command`");
        }
        self.name = self.config.name.clone().unwrap_or_else(|| host.to_string());
        self.url = url;
        self.client = Some(http::client()?);
        Ok(manifest("http", &self.name, 60_000, 2_000, Size::Medium, 0))
    }

    async fn render(&mut self) -> Result<View> {
        let client = self.client.as_ref().context("not started")?;
        let mut req = client
            .get(&self.url)
            .timeout(Duration::from_secs(self.config.timeout_secs));
        for (k, v) in &self.config.headers {
            req = req.header(k, v);
        }
        if self.config.secret.token_env.is_some() || self.config.secret.token_command.is_some() {
            let token = self.config.secret.token("").await?;
            req = match &self.config.token_header {
                Some(header) => req.header(header, token),
                None => req.bearer_auth(token),
            };
        }
        if let Some((etag, _)) = &self.cached {
            req = req.header("If-None-Match", etag);
        }
        let resp = req.send().await.context("fetching")?;
        if resp.status().as_u16() == 304
            && let Some((_, view)) = &self.cached
        {
            return Ok(view.clone());
        }
        let resp = http::check(resp).await?;
        if resp.content_length().is_some_and(|n| n > MAX_BYTES) {
            bail!("the response is over 1 MB");
        }
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let etag = header("etag");
        let markdown = self.config.markdown.unwrap_or_else(|| {
            header("content-type").is_some_and(|t| t.starts_with("text/markdown"))
        });
        let body = resp.bytes().await.context("reading the response")?;
        if body.len() as u64 > MAX_BYTES {
            bail!("the response is over 1 MB");
        }
        let shape = Shape {
            format: self.config.format,
            fields: &self.config.fields,
            markdown,
            lines: self.config.lines,
        };
        let view = feed::view(&String::from_utf8_lossy(&body), &self.name, &shape)?;
        self.cached = etag.map(|e| (e, view.clone()));
        Ok(view)
    }
}

/// The host of an http(s) URL, without user info or port. None for other schemes.
fn host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => host.split(':').next()?,
    };
    (!host.is_empty()).then_some(host)
}

/// The demo: fields picked out of the `queue.json` sample.
const DEMO: &str = include_str!("../testdata/feeds/queue.json");

fn demo_fields() -> Vec<Field> {
    let field = |label: &str, pointer: &str, unit: Option<&str>| Field {
        label: label.into(),
        pointer: pointer.into(),
        unit: unit.map(Into::into),
    };
    vec![
        field("Waiting", "/queue/waiting", None),
        field("Workers", "/workers", None),
        field("p99", "/latency/p99", Some("ms")),
    ]
}

pub fn catalog() -> CatalogEntry {
    let fields = demo_fields();
    let shape = Shape {
        format: Format::Auto,
        fields: &fields,
        markdown: false,
        lines: 200,
    };
    CatalogEntry {
        description: "Shows what a URL returns, fetched again on each refresh: a view or \
                      widget in the dashboard's JSON, values picked out of any JSON, or \
                      text. Add it several times with `builtin: http`."
            .into(),
        tags: crate::strings(&["http", "json", "api", "custom"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry(
            "http",
            "URL",
            Size::Medium,
            feed::view(DEMO, "Queue", &shape).expect("the demo sample has the fields"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{Status, Widget};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn hosts() {
        assert_eq!(host("https://example.com/x"), Some("example.com"));
        assert_eq!(host("http://u:p@127.0.0.1:9000/s?q"), Some("127.0.0.1"));
        assert_eq!(host("http://[::1]:80/"), Some("::1"));
        assert_eq!(host("ftp://example.com"), None);
        assert_eq!(host("https://"), None);
    }

    /// A server that answers each request with the next response in `bodies`
    /// and keeps the requests it got.
    async fn serve(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/stats", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = vec![];
            for response in responses {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 4096];
                let n = sock.read(&mut buf).await.unwrap();
                requests.push(String::from_utf8_lossy(&buf[..n]).into_owned());
                sock.write_all(response.as_bytes()).await.unwrap();
                sock.shutdown().await.ok();
            }
            requests
        });
        (url, task)
    }

    fn ok(headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn fetches_views_and_reuses_them_on_304() {
        let body = r#"{"type":"gauge","label":"Disk","percent":40}"#;
        let (url, server) = serve(vec![
            ok("ETag: \"v1\"\r\n", body),
            "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".into(),
        ])
        .await;
        let mut m = Http::default();
        let manifest = m
            .init(serde_json::json!({ "url": url, "headers": { "Accept": "application/json" } }))
            .await
            .unwrap();
        assert_eq!(manifest.name, "127.0.0.1");
        let first = m.render().await.unwrap();
        assert!(matches!(first.root, Widget::Gauge { .. }));
        assert_eq!(m.render().await.unwrap(), first);
        let requests = server.await.unwrap();
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("accept: application/json")
        );
        assert!(requests[1].contains("\"v1\""), "{}", requests[1]);
    }

    #[tokio::test]
    async fn sends_the_token_and_picks_fields() {
        // SAFETY: only this test reads this variable.
        unsafe { std::env::set_var("DASHBOARD_HTTP_TEST_TOKEN", "t0k") };
        let (url, server) = serve(vec![ok("", DEMO), ok("", DEMO)]).await;
        let fields = serde_json::to_value(
            demo_fields()
                .iter()
                .map(|f| serde_json::json!({ "label": f.label, "pointer": f.pointer }))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut m = Http::default();
        m.init(serde_json::json!({
            "url": url, "name": "Queue", "fields": fields,
            "token_env": "DASHBOARD_HTTP_TEST_TOKEN",
        }))
        .await
        .unwrap();
        let v = m.render().await.unwrap();
        assert_eq!(v.status, Status::Ok);
        assert!(v.summary.starts_with("Waiting 14"), "{}", v.summary);

        let mut m = Http::default();
        m.init(serde_json::json!({
            "url": url, "token_env": "DASHBOARD_HTTP_TEST_TOKEN", "token_header": "X-Api-Key",
        }))
        .await
        .unwrap();
        m.render().await.unwrap();
        let requests = server.await.unwrap();
        assert!(
            requests[0].contains("authorization: Bearer t0k"),
            "{}",
            requests[0]
        );
        assert!(
            requests[1].to_ascii_lowercase().contains("x-api-key: t0k"),
            "{}",
            requests[1]
        );
    }

    #[tokio::test]
    async fn http_errors_show() {
        let (url, server) = serve(vec![
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\noops"
                .into(),
        ])
        .await;
        let mut m = Http::default();
        m.init(serde_json::json!({ "url": url })).await.unwrap();
        let err = m.render().await.unwrap_err().to_string();
        assert!(err.contains("500") && err.contains("oops"), "{err}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn config_errors() {
        for config in [
            serde_json::json!({}),
            serde_json::json!({ "url": "example.com" }),
            serde_json::json!({ "url": "https://x", "timeout_secs": 0 }),
            serde_json::json!({ "url": "https://x", "headers": { "Authorization": "Bearer x" } }),
            serde_json::json!({ "url": "https://x", "token_header": "X-Key" }),
            serde_json::json!({ "url": "https://x", "format": "text", "fields": [{ "label": "a", "pointer": "/a" }] }),
        ] {
            assert!(
                Http::default().init(config.clone()).await.is_err(),
                "{config}"
            );
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
