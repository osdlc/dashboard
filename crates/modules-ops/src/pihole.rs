//! Pi-hole: queries and blocked queries today, active clients, whether blocking
//! is on, and the most blocked domains. Reads the local REST API of Pi-hole v6
//! (`/api/stats/summary`, `/api/dns/blocking`, `/api/stats/top_domains`).
//!
//! ```yaml
//! modules:
//!   pihole:
//!     config:
//!       url: http://pi.hole           # the Pi-hole web server
//!       token_env: PIHOLE_PASSWORD    # its password or an app password, if it has one
//!       top: 5                        # most blocked domains shown, 0 for none
//! ```
//!
//! With a password, the module logs in once (`POST /api/auth`) and keeps the
//! session, logging in again when it expires. An app password (Settings >
//! Web interface / API) is better than the main one. Untested live: built
//! from the API docs with sample responses.

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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The Pi-hole web server, e.g. `http://pi.hole` or `http://192.168.1.2:8080`.
    url: String,
    /// The password: `token_env` (default `PIHOLE_PASSWORD`) or `token_command`.
    /// Leave both unset for a Pi-hole without a password.
    #[serde(flatten)]
    secret: Secret,
    /// Most blocked domains shown. 0 hides the list.
    top: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: "http://pi.hole".into(),
            secret: Secret::default(),
            top: 5,
        }
    }
}

#[derive(Default)]
pub struct Pihole {
    config: Config,
    base: String,
    /// The session from the last login, reused until the API says it expired.
    sid: Option<String>,
    /// True after a login worked, even one that gave no session id (a
    /// Pi-hole without a password), so the next renders skip the login.
    logged_in: bool,
}

#[async_trait]
impl Module for Pihole {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        // A reload ends the old session first.
        self.logout().await;
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let url = self.config.url.trim_end_matches('/');
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("url: expected http://... or https://..., got {url:?}");
        }
        self.base = url.to_string();
        Ok(manifest(
            "pihole",
            "Pi-hole",
            30_000,
            10_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let http = client()?;
        if !self.logged_in {
            self.login(&http).await?;
        }
        // A kept session can expire; then log in again once.
        let fetched = match self.fetch(&http).await {
            Err(e) if self.logged_in && is_unauthorized(&e) => {
                self.sid = None;
                self.logged_in = false;
                self.login(&http).await?;
                self.fetch(&http).await
            }
            other => other,
        };
        let (summary, blocking, top) = fetched?;
        Ok(view(&summary, &blocking, top.as_ref(), &self.base))
    }
}

impl Pihole {
    /// `POST /api/auth`. The password is read only here, so a
    /// `token_command` runs once per session, not on every refresh. With no
    /// password set this does nothing, and a Pi-hole that wants one answers
    /// the next request with a 401. A Pi-hole without a password gives no
    /// session id.
    async fn login(&mut self, http: &Client) -> Result<()> {
        let Some(password) = self.config.secret.token_opt("PIHOLE_PASSWORD").await? else {
            return Ok(());
        };
        let resp = http
            .post(format!("{}/api/auth", self.base))
            .json(&serde_json::json!({ "password": password }))
            .send()
            .await
            .with_context(|| format!("reaching Pi-hole at {}", self.base))?;
        if resp.status().as_u16() == 401 {
            bail!("Pi-hole rejected the password");
        }
        let auth: Auth = check(resp).await?.json().await?;
        self.sid = parse_auth(auth)?;
        self.logged_in = true;
        Ok(())
    }

    /// Ends the kept session (`DELETE /api/auth`), so a reload doesn't use
    /// up one of Pi-hole's limited session slots. Best effort: a failure
    /// only means the session times out on its own.
    async fn logout(&mut self) {
        self.logged_in = false;
        let Some(sid) = self.sid.take() else {
            return;
        };
        if let Ok(http) = client() {
            let _ = http
                .delete(format!("{}/api/auth", self.base))
                .header("X-FTL-SID", sid)
                .send()
                .await;
        }
    }

    async fn fetch(&self, http: &Client) -> Result<(Summary, Blocking, Option<TopDomains>)> {
        let summary = self.get(http, "/api/stats/summary").await?;
        let blocking = self.get(http, "/api/dns/blocking").await?;
        let top = if self.config.top > 0 {
            let path = format!(
                "/api/stats/top_domains?blocked=true&count={}",
                self.config.top
            );
            Some(self.get(http, &path).await?)
        } else {
            None
        };
        Ok((summary, blocking, top))
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, http: &Client, path: &str) -> Result<T> {
        let mut req = http.get(format!("{}{path}", self.base));
        if let Some(sid) = &self.sid {
            req = req.header("X-FTL-SID", sid);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("reaching Pi-hole at {}", self.base))?;
        if resp.status().as_u16() == 401 {
            bail!(Unauthorized);
        }
        Ok(check(resp).await?.json().await?)
    }
}

/// A 401 from the API: no session, or it expired.
#[derive(Debug)]
struct Unauthorized;

impl std::fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Pi-hole asks for a password: set $PIHOLE_PASSWORD, or token_env / token_command in the config",
        )
    }
}

impl std::error::Error for Unauthorized {}

fn is_unauthorized(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Unauthorized>().is_some()
}

/// `POST /api/auth`.
#[derive(Debug, Deserialize)]
struct Auth {
    session: Session,
}

#[derive(Debug, Deserialize)]
struct Session {
    valid: bool,
    sid: Option<String>,
    #[serde(default)]
    totp: bool,
    message: Option<String>,
}

/// The session id to send, or why there is none.
fn parse_auth(auth: Auth) -> Result<Option<String>> {
    let s = auth.session;
    if !s.valid {
        if s.totp {
            bail!("Pi-hole wants a two-factor code: use an app password instead");
        }
        bail!(
            "Pi-hole login failed: {}",
            s.message.unwrap_or_else(|| "no session".into())
        );
    }
    Ok(s.sid)
}

/// `GET /api/stats/summary` (the parts shown).
#[derive(Debug, Deserialize)]
struct Summary {
    queries: Queries,
    clients: Clients,
}

#[derive(Debug, Deserialize)]
struct Queries {
    total: u64,
    blocked: u64,
    percent_blocked: f64,
}

#[derive(Debug, Deserialize)]
struct Clients {
    active: u64,
}

/// `GET /api/dns/blocking`: "enabled", "disabled", "failed" or "unknown".
#[derive(Debug, Deserialize)]
struct Blocking {
    blocking: String,
    /// Seconds until blocking turns itself back on, when it was paused.
    timer: Option<f64>,
}

/// `GET /api/stats/top_domains?blocked=true`.
#[derive(Debug, Deserialize)]
struct TopDomains {
    domains: Vec<DomainCount>,
}

#[derive(Debug, Deserialize)]
struct DomainCount {
    domain: String,
    count: u64,
}

fn count(n: u64) -> String {
    format_value(n as f64, ValueFormat::Count, None)
}

/// The tile. `top` is None when the list is turned off.
fn view(s: &Summary, b: &Blocking, top: Option<&TopDomains>, base: &str) -> View {
    let on = b.blocking == "enabled";
    let state = match (b.blocking.as_str(), b.timer) {
        ("enabled", _) => "on".to_string(),
        ("disabled", Some(t)) if t > 0.0 => format!("off for {}m", (t / 60.0).ceil()),
        ("disabled", _) => "off".to_string(),
        (other, _) => other.to_string(),
    };
    let pct = format!("{:.1}%", s.queries.percent_blocked);
    let mut children = vec![Widget::Row {
        children: vec![
            Widget::Stat {
                label: "Queries".into(),
                value: count(s.queries.total),
                unit: None,
                delta: None,
                status: None,
            },
            Widget::Stat {
                label: "Blocked".into(),
                value: count(s.queries.blocked),
                unit: None,
                delta: Some(pct.clone()),
                status: None,
            },
            Widget::Stat {
                label: "Clients".into(),
                value: s.clients.active.to_string(),
                unit: None,
                delta: None,
                status: None,
            },
            Widget::Stat {
                label: "Blocking".into(),
                value: state.clone(),
                unit: None,
                delta: None,
                status: Some(if on { Status::Ok } else { Status::Warn }),
            },
        ],
    }];
    if let Some(top) = top {
        children.push(Widget::Text {
            text: "**Most blocked**".into(),
            markdown: true,
        });
        children.push(Widget::List {
            items: top
                .domains
                .iter()
                .map(|d| ListItem {
                    tone: None,
                    title: d.domain.clone(),
                    subtitle: Some(format!("{} blocked", count(d.count))),
                    status: None,
                    tags: vec![],
                    action: Some(Action::OpenUrl {
                        url: format!("{base}/admin/queries?domain={}", d.domain),
                    }),
                })
                .collect(),
            empty_text: Some("nothing blocked yet".into()),
        });
    }
    View {
        status: if on { Status::Ok } else { Status::Warn },
        summary: format!(
            "blocking {state} · {pct} of {} queries blocked",
            count(s.queries.total)
        ),
        root: Widget::Column { children },
    }
}

const SUMMARY: &str = include_str!("../testdata/pihole/summary.json");
const BLOCKING: &str = include_str!("../testdata/pihole/blocking.json");
const TOP: &str = include_str!("../testdata/pihole/top_blocked.json");

pub fn catalog() -> CatalogEntry {
    let summary: Summary = serde_json::from_str(SUMMARY).expect("demo summary parses");
    let blocking: Blocking = serde_json::from_str(BLOCKING).expect("demo blocking parses");
    let top: TopDomains = serde_json::from_str(TOP).expect("demo top domains parse");
    let demo = view(&summary, &blocking, Some(&top), "http://pi.hole");
    CatalogEntry {
        description: "Pi-hole v6 on your network: queries and blocked queries today, active \
                      clients, whether blocking is on, and the most blocked domains. Click a \
                      domain to see its queries. Untested live: built from the API docs with \
                      sample responses."
            .into(),
        tags: crate::strings(&["pihole", "dns", "network", "ops", "local", "untested-live"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: crate::strings(&["PIHOLE_PASSWORD"]),
            hosts: crate::strings(&["pi.hole (your Pi-hole)"]),
        },
        ..crate::entry("pihole", "Pi-hole", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Summary, Blocking, TopDomains) {
        (
            serde_json::from_str(SUMMARY).unwrap(),
            serde_json::from_str(BLOCKING).unwrap(),
            serde_json::from_str(TOP).unwrap(),
        )
    }

    #[test]
    fn parses_the_sample_responses() {
        let (s, b, top) = sample();
        let v = view(&s, &b, Some(&top), "http://pi.hole");
        assert_eq!(v.status, Status::Ok);
        assert_eq!(v.summary, "blocking on · 20.5% of 48.2k queries blocked");
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        let Widget::Row { children: stats } = &children[0] else {
            panic!("row")
        };
        let Widget::Stat {
            value: blocked,
            delta,
            ..
        } = &stats[1]
        else {
            panic!("stat")
        };
        assert_eq!(blocked, "9.9k");
        assert_eq!(delta.as_deref(), Some("20.5%"));
        let Widget::List { items, .. } = &children[2] else {
            panic!("list")
        };
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].title, "telemetry.example-tv.com");
        assert_eq!(items[0].subtitle.as_deref(), Some("2.2k blocked"));
        assert_eq!(
            items[0].action,
            Some(Action::OpenUrl {
                url: "http://pi.hole/admin/queries?domain=telemetry.example-tv.com".into()
            })
        );
    }

    #[test]
    fn paused_blocking_warns() {
        let (s, _, _) = sample();
        let b: Blocking =
            serde_json::from_str(r#"{"blocking":"disabled","timer":290.5,"took":0.0}"#).unwrap();
        let v = view(&s, &b, None, "http://pi.hole");
        assert_eq!(v.status, Status::Warn);
        assert!(
            v.summary.starts_with("blocking off for 5m"),
            "{}",
            v.summary
        );
        let Widget::Column { children } = &v.root else {
            panic!("column")
        };
        assert_eq!(children.len(), 1, "no list when top is 0");
    }

    #[test]
    fn parses_login() {
        let auth: Auth =
            serde_json::from_str(include_str!("../testdata/pihole/auth.json")).unwrap();
        assert_eq!(
            parse_auth(auth).unwrap().as_deref(),
            Some("vFA+EP4MQ5JJvJg+3Q2Jnw=")
        );
        // No password set: valid, no session id.
        let open: Auth = serde_json::from_str(
            r#"{"session":{"valid":true,"totp":false,"sid":null,"validity":-1}}"#,
        )
        .unwrap();
        assert_eq!(parse_auth(open).unwrap(), None);
        let wrong: Auth = serde_json::from_str(
            r#"{"session":{"valid":false,"totp":false,"sid":null,"message":"password incorrect"}}"#,
        )
        .unwrap();
        let err = parse_auth(wrong).unwrap_err().to_string();
        assert!(err.contains("password incorrect"), "{err}");
    }

    #[tokio::test]
    async fn bad_url_is_a_config_error() {
        let mut m = Pihole::default();
        let err = m
            .init(serde_json::json!({ "url": "pi.hole" }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("url"), "{err}");
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
