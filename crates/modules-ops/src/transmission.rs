//! Transmission: torrents from the BitTorrent client's RPC API, with progress,
//! speeds and errors. Errors come first, then downloads, then the rest.
//!
//! ```yaml
//! modules:
//!   transmission:
//!     config:
//!       url: http://localhost:9091        # where the web UI / RPC listens
//!       username: me                      # only if RPC authentication is on
//!       token_env: TRANSMISSION_PASSWORD  # its password
//!       limit: 10
//! ```
//!
//! The RPC API wants a session id header: the first request gets a 409 that
//! carries one, and the module sends it from then on. Untested live: built
//! from the RPC spec with a sample response.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, ValueFormat, View,
    Widget, format_value,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{Client, check, client};
use dashboard_modules_common::manifest;
use dashboard_modules_common::secret::Secret;
use schemars::JsonSchema;
use serde::Deserialize;

const SESSION_HEADER: &str = "X-Transmission-Session-Id";

/// The torrent fields the tile reads.
const FIELDS: &[&str] = &[
    "id",
    "name",
    "status",
    "percentDone",
    "rateDownload",
    "rateUpload",
    "eta",
    "error",
    "errorString",
    "uploadRatio",
    "totalSize",
];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Where Transmission's web UI and RPC listen, e.g. `http://localhost:9091`.
    url: String,
    /// RPC user name, when RPC authentication is on.
    username: Option<String>,
    /// The RPC password: `token_env` (default `TRANSMISSION_PASSWORD`) or `token_command`.
    #[serde(flatten)]
    secret: Secret,
    /// Torrents shown.
    limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: "http://localhost:9091".into(),
            username: None,
            secret: Secret::default(),
            limit: 10,
        }
    }
}

#[derive(Default)]
pub struct Transmission {
    config: Config,
    base: String,
    /// The session id from the last 409.
    session: Option<String>,
    /// The password once read (`Some(None)` when none is set), kept so a
    /// `token_command` doesn't run on every refresh. Dropped after a 401.
    password: Option<Option<String>>,
}

#[async_trait]
impl Module for Transmission {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let url = self.config.url.trim_end_matches('/');
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("url: expected http://... or https://..., got {url:?}");
        }
        // Without a user name the password is never sent, so a password
        // set in the config is a mistake. (The default $TRANSMISSION_PASSWORD
        // may be set for other reasons, so it isn't checked.)
        let secret = &self.config.secret;
        if self.config.username.is_none()
            && (secret.token_env.is_some() || secret.token_command.is_some())
        {
            bail!("username: set it too; Transmission sends the password only with a user name");
        }
        self.base = url.to_string();
        self.session = None;
        self.password = None;
        Ok(manifest(
            "transmission",
            "Transmission",
            10_000,
            3_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let http = client()?;
        // The password is needed only with a user name (basic auth).
        if self.password.is_none() {
            self.password = Some(match self.config.username {
                Some(_) => {
                    self.config
                        .secret
                        .token_opt("TRANSMISSION_PASSWORD")
                        .await?
                }
                None => None,
            });
        }
        let password = self.password.clone().flatten();
        let body = serde_json::json!({
            "method": "torrent-get",
            "arguments": { "fields": FIELDS },
        });
        // The first call (and any after Transmission restarts) gets a 409
        // with the session id to use; then it is sent again.
        let mut resp = self.post(&http, &body, password.as_deref()).await?;
        if resp.status().as_u16() == 409 {
            self.session = resp
                .headers()
                .get(SESSION_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            resp = self.post(&http, &body, password.as_deref()).await?;
        }
        if resp.status().as_u16() == 401 {
            // Read the password again next time, in case it changed.
            self.password = None;
            bail!(
                "Transmission rejected the login: set `username` in the config, and the password in $TRANSMISSION_PASSWORD (or token_env / token_command)"
            );
        }
        let reply: Reply = check(resp).await?.json().await?;
        let torrents = parse(reply)?;
        Ok(view(torrents, self.config.limit, &self.base))
    }
}

impl Transmission {
    async fn post(
        &self,
        http: &Client,
        body: &serde_json::Value,
        password: Option<&str>,
    ) -> Result<dashboard_modules_common::http::Response> {
        let mut req = http
            .post(format!("{}/transmission/rpc", self.base))
            .json(body);
        if let Some(session) = &self.session {
            req = req.header(SESSION_HEADER, session);
        }
        if let Some(user) = &self.config.username {
            req = req.basic_auth(user, password);
        }
        req.send()
            .await
            .with_context(|| format!("reaching Transmission at {}", self.base))
    }
}

/// A `torrent-get` reply.
#[derive(Debug, Deserialize)]
struct Reply {
    result: String,
    #[serde(default)]
    arguments: Option<Arguments>,
}

#[derive(Debug, Deserialize)]
struct Arguments {
    torrents: Vec<Torrent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Torrent {
    name: String,
    /// 0 stopped, 1 queued to check, 2 checking, 3 queued to download,
    /// 4 downloading, 5 queued to seed, 6 seeding.
    status: u8,
    percent_done: f64,
    rate_download: u64,
    rate_upload: u64,
    /// Seconds left, or -1 / -2 when unknown.
    eta: i64,
    /// 0 is no error; 1 and 2 are tracker warnings and errors, 3 a local error.
    error: u8,
    #[serde(default)]
    error_string: String,
    upload_ratio: f64,
    total_size: u64,
}

fn parse(reply: Reply) -> Result<Vec<Torrent>> {
    if reply.result != "success" {
        bail!("Transmission: {}", reply.result);
    }
    Ok(reply
        .arguments
        .context("Transmission sent no torrents")?
        .torrents)
}

/// Sort key: errors, downloading, checking, queued, seeding, stopped.
fn rank(t: &Torrent) -> u8 {
    if t.error != 0 {
        return 0;
    }
    match t.status {
        4 => 1,
        1 | 2 => 2,
        3 | 5 => 3,
        6 => 4,
        _ => 5,
    }
}

fn bytes(n: u64) -> String {
    format_value(n as f64, ValueFormat::Bytes, None)
}

fn rate(n: u64) -> String {
    format_value(n as f64, ValueFormat::Bytes, Some("/s"))
}

/// "9m", "2h 5m", "3d".
fn eta(secs: i64) -> String {
    match secs {
        s if s < 60 => "<1m".into(),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h {}m", s / 3600, s / 60 % 60),
        s => format!("{}d", s / 86_400),
    }
}

fn item(t: &Torrent, web: &str) -> ListItem {
    let pct = format!("{:.0}%", (t.percent_done * 100.0).floor());
    let mut sub = vec![match t.status {
        0 if t.percent_done >= 1.0 => "done".to_string(),
        0 => format!("stopped at {pct}"),
        1 | 2 => "checking".to_string(),
        3 => "queued".to_string(),
        4 => format!("downloading {pct}"),
        5 | 6 => "seeding".to_string(),
        n => format!("status {n}"),
    }];
    sub.push(bytes(t.total_size));
    if t.rate_download > 0 {
        sub.push(format!("↓ {}", rate(t.rate_download)));
    }
    if t.rate_upload > 0 {
        sub.push(format!("↑ {}", rate(t.rate_upload)));
    }
    if t.status == 4 && t.eta >= 0 {
        sub.push(format!("{} left", eta(t.eta)));
    }
    // -1 means no ratio yet, -2 an infinite one.
    if t.percent_done >= 1.0 {
        if t.upload_ratio == -2.0 {
            sub.push("ratio ∞".into());
        } else if t.upload_ratio >= 0.0 {
            sub.push(format!("ratio {:.2}", t.upload_ratio));
        }
    }
    let mut subtitle = sub.join(" · ");
    if t.error != 0 && !t.error_string.is_empty() {
        subtitle = format!("{} · {subtitle}", t.error_string);
    }
    ListItem {
        tone: None,
        title: t.name.clone(),
        subtitle: Some(subtitle),
        status: Some(match (t.error, t.status) {
            (3, _) | (2, _) => Status::Error,
            (1, _) => Status::Warn,
            (_, 4..=6) => Status::Ok,
            _ => Status::Unknown,
        }),
        tags: vec![],
        action: Some(Action::OpenUrl { url: web.into() }),
    }
}

/// The tile: total speeds, then the torrents.
fn view(mut torrents: Vec<Torrent>, limit: usize, base: &str) -> View {
    torrents.sort_by_key(rank);
    let web = format!("{base}/transmission/web/");
    let down: u64 = torrents.iter().map(|t| t.rate_download).sum();
    let up: u64 = torrents.iter().map(|t| t.rate_upload).sum();
    let downloading = torrents.iter().filter(|t| t.status == 4).count();
    let seeding = torrents.iter().filter(|t| t.status == 6).count();
    let failed = torrents.iter().filter(|t| t.error >= 2).count();

    let mut summary = vec![
        format!("{downloading} downloading"),
        format!("{seeding} seeding"),
    ];
    if failed > 0 {
        summary.push(format!("{failed} with errors"));
    }
    summary.push(format!("↓ {} ↑ {}", rate(down), rate(up)));
    let stat = |label: &str, value: String| Widget::Stat {
        label: label.into(),
        value,
        unit: None,
        delta: None,
        status: None,
    };
    View {
        status: if failed > 0 {
            Status::Error
        } else {
            Status::Ok
        },
        summary: summary.join(" · "),
        root: Widget::Column {
            children: vec![
                Widget::Row {
                    children: vec![
                        stat("Down", rate(down)),
                        stat("Up", rate(up)),
                        stat("Torrents", torrents.len().to_string()),
                    ],
                },
                Widget::List {
                    items: torrents.iter().take(limit).map(|t| item(t, &web)).collect(),
                    empty_text: Some("no torrents".into()),
                },
            ],
        },
    }
}

const SAMPLE: &str = include_str!("../testdata/transmission/torrent-get.json");

pub fn catalog() -> CatalogEntry {
    let reply: Reply = serde_json::from_str(SAMPLE).expect("demo reply parses");
    let demo = view(
        parse(reply).expect("demo reply is a success"),
        10,
        "http://localhost:9091",
    );
    CatalogEntry {
        description: "Torrents in Transmission: progress, speeds, time left, ratio and errors, \
                      from its RPC API. Click one to open the web UI. Untested live: built from \
                      the RPC spec with a sample response."
            .into(),
        tags: crate::strings(&[
            "transmission",
            "torrents",
            "downloads",
            "ops",
            "local",
            "untested-live",
        ]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: crate::strings(&["TRANSMISSION_PASSWORD"]),
            hosts: crate::strings(&["localhost:9091 (your Transmission)"]),
        },
        ..crate::entry("transmission", "Transmission", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Torrent> {
        parse(serde_json::from_str(SAMPLE).unwrap()).unwrap()
    }

    #[test]
    fn parses_the_sample_reply() {
        let v = view(sample(), 10, "http://localhost:9091");
        assert_eq!(v.status, Status::Error);
        assert_eq!(
            v.summary,
            "1 downloading · 1 seeding · 1 with errors · ↓ 6.2 MiB/s ↑ 218.0 KiB/s"
        );
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::List { items, .. } = &children[1] else {
            panic!("list")
        };
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "archlinux-2026.10.01-x86_64.iso",
                "ubuntu-26.04-desktop-amd64.iso",
                "fedora-workstation-43-x86_64.iso",
                "debian-13.1.0-amd64-netinst.iso",
                "Big Buck Bunny (2008) [1080p]",
            ]
        );
        assert_eq!(items[0].status, Some(Status::Error));
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("No data found! Ensure your drives are connected · stopped at 13% · 1.2 GiB")
        );
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("downloading 42% · 5.8 GiB · ↓ 6.2 MiB/s · ↑ 40.0 KiB/s · 8m left")
        );
        assert_eq!(
            items[3].subtitle.as_deref(),
            Some("seeding · 754.0 MiB · ↑ 178.0 KiB/s · ratio 2.31")
        );
        assert_eq!(
            items[4].subtitle.as_deref(),
            Some("done · 885.6 MiB · ratio 1.07")
        );
        assert_eq!(
            items[0].action,
            Some(Action::OpenUrl {
                url: "http://localhost:9091/transmission/web/".into()
            })
        );
    }

    #[test]
    fn failed_reply_is_an_error() {
        let reply: Reply =
            serde_json::from_str(r#"{"result":"no permission to access the torrent"}"#).unwrap();
        assert!(parse(reply).unwrap_err().to_string().contains("permission"));
    }

    #[test]
    fn limit_keeps_the_totals() {
        let v = view(sample(), 1, "http://localhost:9091");
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::List { items, .. } = &children[1] else {
            panic!("list")
        };
        assert_eq!(items.len(), 1);
        assert!(v.summary.starts_with("1 downloading · 1 seeding"));
    }

    #[test]
    fn etas() {
        assert_eq!(eta(30), "<1m");
        assert_eq!(eta(532), "8m");
        assert_eq!(eta(7500), "2h 5m");
        assert_eq!(eta(200_000), "2d");
    }

    #[test]
    fn ratio_and_queued_seeding() {
        let mut t = sample()
            .into_iter()
            .find(|t| t.percent_done >= 1.0 && t.status == 0)
            .unwrap();
        t.upload_ratio = -1.0;
        assert!(!item(&t, "w").subtitle.unwrap().contains("ratio"));
        t.upload_ratio = -2.0;
        assert!(item(&t, "w").subtitle.unwrap().ends_with("ratio ∞"));
        t.status = 5;
        assert_eq!(item(&t, "w").status, Some(Status::Ok));
    }

    #[tokio::test]
    async fn password_without_username_is_a_config_error() {
        let mut m = Transmission::default();
        let err = m
            .init(serde_json::json!({ "token_env": "X" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("username"), "{err}");
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
