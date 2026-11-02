//! Uptime checks, run with `curl`. Each check is a URL or a full curl command:
//!
//! ```yaml
//! modules:
//!   uptime:
//!     config:
//!       checks:
//!         - { name: github, url: https://github.com }
//!         - name: api health
//!           # Run through `sh`, so $VARS expand: keep tokens in the environment.
//!           # On Windows there is no shell: see `split_words`.
//!           curl: 'curl -H "Authorization: Bearer $API_TOKEN" https://api.example.com/health'
//!           expect: 200          # one code or a list; default any 2xx or 3xx
//!           slow_ms: 800         # slower than this counts as a warning
//! ```

use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, Manifest, Requirements, SeriesData, Size, Status, UptimeCheck, View, Widget,
};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;
use tokio::task::JoinSet;

use dashboard_modules_common::manifest;

/// Appended to every request: discard the body, print the code and time last.
const WRITE_OUT: &str = "\n%{http_code} %{time_total}";

/// Where curl writes the body.
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    checks: Vec<CheckConfig>,
    /// Seconds before a check gives up.
    timeout: u64,
    /// Results kept per check, for the strip and the uptime %.
    history: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            checks: vec![],
            timeout: 10,
            history: 120,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CheckConfig {
    name: String,
    /// Checked with `curl -L <url>`.
    url: Option<String>,
    /// A full `curl ...` command, run with `sh -c`. On Windows it is split
    /// into words and run without a shell: quotes group words, and `$VAR`,
    /// `${VAR}` and `%VAR%` expand.
    curl: Option<String>,
    #[serde(default)]
    expect: Option<Expect>,
    #[serde(default)]
    slow_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum Expect {
    One(u16),
    Many(Vec<u16>),
}

impl CheckConfig {
    fn passes(&self, code: u16) -> bool {
        match &self.expect {
            None => (200..400).contains(&code),
            Some(Expect::One(c)) => code == *c,
            Some(Expect::Many(cs)) => cs.contains(&code),
        }
    }

    /// The URL shown as a link: `url`, or the first http(s) word of `curl`.
    fn link(&self) -> Option<String> {
        self.url.clone().or_else(|| {
            self.curl.as_deref()?.split_whitespace().find_map(|w| {
                let w = w.trim_matches(|c| c == '"' || c == '\'');
                (w.starts_with("http://") || w.starts_with("https://")).then(|| w.to_string())
            })
        })
    }
}

/// One finished check.
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    status: Status,
    latency_ms: Option<f64>,
    detail: String,
}

#[derive(Default)]
pub struct Uptime {
    config: Config,
    history: Vec<VecDeque<Status>>,
}

#[async_trait]
impl Module for Uptime {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        if self.config.checks.is_empty() {
            bail!("the uptime module needs `config: {{ checks: [{{ name, url }}] }}`");
        }
        for c in &self.config.checks {
            match (&c.url, &c.curl) {
                (Some(_), None) => {}
                (None, Some(cmd)) if cmd.trim_start().starts_with("curl ") => {}
                (None, Some(_)) => bail!("check {:?}: `curl` must start with `curl `", c.name),
                _ => bail!("check {:?}: give either `url` or `curl`", c.name),
            }
        }
        self.history = vec![VecDeque::new(); self.config.checks.len()];
        Ok(manifest(
            "uptime",
            "Uptime",
            30_000,
            5_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let mut set = JoinSet::new();
        for (i, c) in self.config.checks.iter().enumerate() {
            let (c, timeout) = (c.clone(), self.config.timeout);
            set.spawn(async move { (i, run(&c, timeout).await) });
        }
        let mut outcomes = vec![None; self.config.checks.len()];
        while let Some(Ok((i, o))) = set.join_next().await {
            outcomes[i] = Some(o);
        }

        let now = now_ms();
        let mut checks = vec![];
        let mut latency = vec![];
        for (i, c) in self.config.checks.iter().enumerate() {
            let o = outcomes[i].clone().unwrap_or(Outcome {
                status: Status::Unknown,
                latency_ms: None,
                detail: "check did not finish".into(),
            });
            let h = &mut self.history[i];
            h.push_back(o.status);
            while h.len() > self.config.history.max(1) {
                h.pop_front();
            }
            let up = h.iter().filter(|s| **s != Status::Error).count();
            if let Some(ms) = o.latency_ms {
                latency.push(SeriesData {
                    tone: None,
                    name: c.name.clone(),
                    points: vec![(now, ms)],
                });
            }
            checks.push(UptimeCheck {
                name: c.name.clone(),
                status: o.status,
                uptime: Some(100.0 * up as f64 / h.len() as f64),
                latency_ms: o.latency_ms,
                detail: Some(o.detail),
                history: h.iter().copied().collect(),
                url: c.link(),
            });
        }

        Ok(view(checks, latency, Some(self.config.history as u32)))
    }
}

/// The tile for these checks. `history` is how many latency points the
/// client keeps; the demo has its full series already and passes `None`.
fn view(checks: Vec<UptimeCheck>, latency: Vec<SeriesData>, history: Option<u32>) -> View {
    let down = checks.iter().filter(|c| c.status == Status::Error).count();
    let slow = checks.iter().filter(|c| c.status == Status::Warn).count();
    let status = if down > 0 {
        Status::Error
    } else if slow > 0 {
        Status::Warn
    } else {
        Status::Ok
    };
    let mut summary = format!("{} of {} up", checks.len() - down, checks.len());
    if slow > 0 {
        summary.push_str(&format!(" · {slow} slow"));
    }
    View {
        status,
        summary,
        root: Widget::Column {
            children: vec![
                Widget::Uptime { checks },
                Widget::Series {
                    label: "Response time".into(),
                    unit: Some("ms".into()),
                    y_max: None,
                    id: history.map(|_| "latency".into()),
                    history,
                    style: Default::default(),
                    color: Default::default(),
                    mirrored: false,
                    format: Default::default(),
                    series: latency,
                },
            ],
        },
    }
}

async fn run(c: &CheckConfig, timeout: u64) -> Outcome {
    let limit = ["--max-time".to_string(), timeout.to_string()];
    let mut cmd = match (&c.url, &c.curl) {
        (Some(url), _) => {
            let mut cmd = Command::new("curl");
            cmd.args(["-sS", "-L", "-o", NULL_DEVICE, "-w", WRITE_OUT])
                .args(&limit)
                .arg(url);
            cmd
        }
        // No `sh` on Windows, and `cmd /C` would mangle quotes and `%`: run
        // curl itself with the words of the line.
        (None, Some(line)) if cfg!(windows) => {
            let words = match split_words(line, |name| std::env::var(name).ok()) {
                Ok(words) => words,
                Err(e) => return failed(e),
            };
            let mut cmd = Command::new("curl");
            cmd.args(&words[1..])
                .args(["-sS", "-o", NULL_DEVICE, "-w", WRITE_OUT])
                .args(&limit);
            cmd
        }
        (None, Some(line)) => {
            // Our flags go last so they win over the same flags in the command.
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(format!(
                "{line} -sS -o /dev/null -w '{WRITE_OUT}' {}",
                limit.join(" ")
            ));
            cmd
        }
        (None, None) => unreachable!("checked in init"),
    };
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    // A little longer than curl's own limit, in case curl itself hangs.
    let out = match tokio::time::timeout(Duration::from_secs(timeout + 5), cmd.output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return failed(format!("running curl: {e}")),
        Err(_) => return failed("timed out".into()),
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    judge(c, &String::from_utf8_lossy(&out.stdout), stderr.trim())
}

/// A command line split into words the way `sh` would, for the cases a curl
/// line needs: `'...'` keeps text as it is; `"..."` groups words and still
/// expands variables (`\"` is a quote inside it); `$VAR`, `${VAR}` and
/// `%VAR%` outside single quotes become the variable's value (`$VAR` is empty
/// if unset, `%VAR%` stays).
/// A backslash elsewhere is kept, so Windows paths work.
fn split_words(line: &str, var: impl Fn(&str) -> Option<String>) -> Result<Vec<String>, String> {
    let mut words = vec![];
    let mut word = String::new();
    // A word has started, even if it is empty (`""`).
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    let name_char = |c: &char| c.is_ascii_alphanumeric() || *c == '_';
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (Some('"'), '\\') if chars.peek() == Some(&'"') => {
                word.push('"');
                chars.next();
            }
            (_, '$') if chars.peek() == Some(&'{') => {
                chars.next();
                let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                word.push_str(&var(&name).unwrap_or_default());
                started = true;
            }
            (_, '$') if chars.peek().is_some_and(name_char) => {
                let mut name = String::new();
                while let Some(c) = chars.next_if(name_char) {
                    name.push(c);
                }
                word.push_str(&var(&name).unwrap_or_default());
                started = true;
            }
            (_, '%') => {
                // `%NAME%` of a set variable, as `cmd` does; anything else
                // (`%20` in a URL, an unset name) stays as it is.
                let name: String = chars.clone().take_while(|c| *c != '%').collect();
                let closed = chars.clone().nth(name.chars().count()) == Some('%');
                let value = (closed
                    && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                    && name.chars().all(|c| name_char(&c)))
                .then(|| var(&name))
                .flatten();
                match value {
                    Some(value) => {
                        for _ in 0..=name.chars().count() {
                            chars.next();
                        }
                        word.push_str(&value);
                    }
                    None => word.push('%'),
                }
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if let Some(q) = quote {
        return Err(format!("the curl command has an unclosed {q}"));
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Reads curl's write-out line (the last line of stdout).
fn judge(c: &CheckConfig, stdout: &str, stderr: &str) -> Outcome {
    let parsed = stdout.lines().last().and_then(|l| {
        let (code, time) = l.trim().split_once(' ')?;
        Some((
            code.parse::<u16>().ok()?,
            time.parse::<f64>().ok()? * 1000.0,
        ))
    });
    match parsed {
        // 000: no HTTP response at all (DNS, refused, TLS, timeout).
        Some((0, _)) | None => failed(curl_error(stderr)),
        Some((code, ms)) if !c.passes(code) => Outcome {
            status: Status::Error,
            latency_ms: Some(ms),
            detail: format!("HTTP {code}"),
        },
        Some((code, ms)) => Outcome {
            status: if c.slow_ms.is_some_and(|s| ms > s) {
                Status::Warn
            } else {
                Status::Ok
            },
            latency_ms: Some(ms),
            detail: format!("HTTP {code}"),
        },
    }
}

/// `curl: (7) Failed to connect to ...` -> `Failed to connect to ...`
fn curl_error(stderr: &str) -> String {
    let line = stderr.lines().last().unwrap_or("").trim();
    let line = line.strip_prefix("curl: ").unwrap_or(line);
    let line = match line.split_once(") ") {
        Some((code, rest)) if code.starts_with('(') => rest,
        _ => line,
    };
    if line.is_empty() {
        "no response".into()
    } else {
        line.to_string()
    }
}

fn failed(detail: String) -> Outcome {
    Outcome {
        status: Status::Error,
        latency_ms: None,
        detail,
    }
}

pub fn catalog() -> CatalogEntry {
    use crate::{demo_series, demo_values};
    let strip = |fails: &[usize], slow: &[usize]| -> Vec<Status> {
        (0..60)
            .map(|i| {
                if fails.contains(&i) {
                    Status::Error
                } else if slow.contains(&i) {
                    Status::Warn
                } else {
                    Status::Ok
                }
            })
            .collect()
    };
    let check = |name: &str, url: &str, status, uptime, ms, history| UptimeCheck {
        name: name.into(),
        status,
        uptime: Some(uptime),
        latency_ms: Some(ms),
        detail: Some("HTTP 200".into()),
        history,
        url: Some(url.into()),
    };
    let checks = vec![
        check(
            "website",
            "https://example.com",
            Status::Ok,
            100.0,
            182.0,
            strip(&[], &[]),
        ),
        check(
            "api",
            "https://api.example.com/health",
            Status::Ok,
            98.3,
            241.0,
            strip(&[17], &[16, 18]),
        ),
        check(
            "docs",
            "https://docs.example.com",
            Status::Warn,
            100.0,
            912.0,
            strip(&[], &[55, 58, 59]),
        ),
    ];
    let latency = vec![
        demo_series("website", 30_000, &demo_values(60, 180.0, 30.0, 0.0)),
        demo_series("api", 30_000, &demo_values(60, 240.0, 60.0, 2.0)),
        demo_series("docs", 30_000, &demo_values(60, 600.0, 300.0, 4.0)),
    ];
    let demo = view(checks, latency, None);
    CatalogEntry {
        description: "Checks that websites and APIs answer, with uptime and response times. \
                      Each check is a URL or a full curl command."
            .into(),
        tags: crate::strings(&["monitoring", "network", "ops"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["curl"]),
            env: vec![],
            hosts: crate::strings(&["the URLs you check"]),
        },
        ..crate::entry("uptime", "Uptime", Size::Medium, demo)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(expect: Option<Expect>, slow_ms: Option<f64>) -> CheckConfig {
        CheckConfig {
            name: "x".into(),
            url: Some("https://example.com".into()),
            curl: None,
            expect,
            slow_ms,
        }
    }

    #[test]
    fn judges_codes_and_speed() {
        let ok = judge(&check(None, None), "\n200 0.120", "");
        assert_eq!(ok.status, Status::Ok);
        assert_eq!(ok.latency_ms, Some(120.0));
        assert_eq!(
            judge(&check(None, None), "\n503 0.1", "").status,
            Status::Error
        );
        assert_eq!(
            judge(&check(Some(Expect::One(503)), None), "\n503 0.1", "").status,
            Status::Ok
        );
        assert_eq!(
            judge(&check(None, Some(100.0)), "\n200 0.3", "").status,
            Status::Warn
        );
    }

    #[test]
    fn no_response_shows_curl_error() {
        let o = judge(
            &check(None, None),
            "\n000 0.001",
            "curl: (7) Failed to connect to 127.0.0.1 port 9 after 0 ms: Couldn't connect to server",
        );
        assert_eq!(o.status, Status::Error);
        assert!(o.detail.starts_with("Failed to connect"), "{}", o.detail);
    }

    #[test]
    fn link_comes_from_curl_command() {
        let c = CheckConfig {
            url: None,
            curl: Some("curl -H 'A: b' \"https://api.example.com/health\"".into()),
            ..check(None, None)
        };
        assert_eq!(c.link().as_deref(), Some("https://api.example.com/health"));
    }

    #[test]
    fn splits_curl_lines_without_a_shell() {
        let var = |name: &str| (name == "TOKEN").then(|| "s3cret".to_string());
        let words = |line: &str| split_words(line, var).unwrap();
        assert_eq!(
            words(r#"curl -H "Authorization: Bearer $TOKEN" https://x.test/health"#),
            [
                "curl",
                "-H",
                "Authorization: Bearer s3cret",
                "https://x.test/health"
            ]
        );
        assert_eq!(
            words("curl -H 'A: $TOKEN'  ''"),
            ["curl", "-H", "A: $TOKEN", ""]
        );
        assert_eq!(
            words("curl -u me:${TOKEN} %TOKEN%"),
            ["curl", "-u", "me:s3cret", "s3cret"]
        );
        assert_eq!(words("curl $MISSING x"), ["curl", "", "x"]);
        // A `%` that isn't a set variable, a `$` alone and a Windows path stay.
        assert_eq!(
            words(r#"curl https://x.test/a%E2%80%99b?p=50% %NOPE% $ --cacert C:\certs\ca.pem"#),
            [
                "curl",
                "https://x.test/a%E2%80%99b?p=50%",
                "%NOPE%",
                "$",
                "--cacert",
                r"C:\certs\ca.pem"
            ]
        );
        assert_eq!(
            words(r#"curl -d "say \"hi\"""#),
            ["curl", "-d", r#"say "hi""#]
        );
        assert!(split_words("curl 'oops", var).is_err());
    }

    #[tokio::test]
    async fn refused_port_is_down() {
        let mut m = Uptime::default();
        m.init(serde_json::json!({ "checks": [{ "name": "closed", "url": "http://127.0.0.1:9" }], "timeout": 2 }))
            .await
            .unwrap();
        let view = m.render().await.unwrap();
        assert_eq!(view.status, Status::Error);
        assert_eq!(view.summary, "0 of 1 up");
    }
}
