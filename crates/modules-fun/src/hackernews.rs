//! Hacker News: the top stories (or new, best, Ask HN, Show HN, jobs) from the
//! public HN Firebase API, as a list. Clicking a story opens its link, or its
//! HN page when it has none. No account or key.
//!
//! ```yaml
//! modules:
//!   hackernews:
//!     config: { feed: top, limit: 10 }
//! ```
//!
//! A native-vs-plugin benchmark widget: `plugins/ext/hackernews-ts` and
//! `plugins/ext/hackernews-py` follow the same rules and must show the same
//! view for the same input (`tests/hackernews_same_view.rs`).

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{ago, plural};

const API: &str = "https://hacker-news.firebaseio.com/v0";

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Feed {
    Top,
    New,
    Best,
    Ask,
    Show,
    Job,
}

impl Feed {
    /// The API's list of story ids for this feed.
    fn path(self) -> &'static str {
        match self {
            Feed::Top => "topstories",
            Feed::New => "newstories",
            Feed::Best => "beststories",
            Feed::Ask => "askstories",
            Feed::Show => "showstories",
            Feed::Job => "jobstories",
        }
    }

    /// For the summary: "10 top stories".
    fn label(self) -> &'static str {
        match self {
            Feed::Top => "top stories",
            Feed::New => "new stories",
            Feed::Best => "best stories",
            Feed::Ask => "Ask HN posts",
            Feed::Show => "Show HN posts",
            Feed::Job => "jobs",
        }
    }
}

/// Most stories shown. Each one is a request per refresh, so keep it small.
const MAX_LIMIT: usize = 30;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    feed: Feed,
    /// Stories shown, at most 30 (one HN page).
    #[schemars(range(max = 30))]
    limit: usize,
    /// Read this file instead of calling the API: a JSON array of HN items
    /// (`/v0/item/<id>.json` replies, in feed order). For tests and demos.
    #[schemars(extend("x-path" = true))]
    sample: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            feed: Feed::Top,
            limit: 10,
            sample: None,
        }
    }
}

#[derive(Default)]
pub struct HackerNews {
    config: Config,
}

#[async_trait]
impl Module for HackerNews {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        if self.config.limit > MAX_LIMIT {
            bail!("limit: at most {MAX_LIMIT}");
        }
        Ok(manifest(
            "hackernews",
            "Hacker News",
            300_000,
            60_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let items = match &self.config.sample {
            Some(path) => read_sample(path)?,
            None => fetch(self.config.feed, self.config.limit).await?,
        };
        let now = dashboard_host::now_ms() / 1000;
        Ok(view(&items, self.config.feed, self.config.limit, now))
    }
}

fn read_sample(path: &str) -> Result<Vec<Value>> {
    let path = dashboard_modules_common::expand_home(path);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    match serde_json::from_str(&text) {
        Ok(Value::Array(items)) => Ok(items),
        _ => bail!("{}: expected a JSON array of items", path.display()),
    }
}

/// The feed's first `limit` items, fetched at the same time, in feed order.
async fn fetch(feed: Feed, limit: usize) -> Result<Vec<Value>> {
    let http = client()?;
    let url = format!("{API}/{}.json", feed.path());
    let ids: Vec<u64> = check(http.get(&url).send().await?).await?.json().await?;
    let tasks: Vec<_> = ids
        .into_iter()
        .take(limit)
        .map(|id| {
            let http = http.clone();
            tokio::spawn(async move {
                let resp = http.get(format!("{API}/item/{id}.json")).send().await?;
                anyhow::Ok(check(resp).await?.json::<Value>().await?)
            })
        })
        .collect();
    let mut items = vec![];
    for task in tasks {
        items.push(task.await??);
    }
    Ok(items)
}

/// One story, from an HN item. Deleted and dead items, and items without a
/// title, give `None`.
fn story(item: &Value, now: i64) -> Option<ListItem> {
    let text = |key: &str| item.get(key).and_then(Value::as_str);
    let int = |key: &str| item.get(key).and_then(Value::as_i64);
    let flag = |key: &str| item.get(key).and_then(Value::as_bool) == Some(true);
    if flag("deleted") || flag("dead") {
        return None;
    }
    let title = text("title")?;
    let url = text("url").filter(|u| !u.is_empty());

    let mut sub = vec![];
    if let Some(n) = int("score") {
        sub.push(plural(n, "point"));
    }
    if let Some(n) = int("descendants") {
        sub.push(plural(n, "comment"));
    }
    if let Some(by) = text("by") {
        sub.push(format!("by {by}"));
    }
    if let Some(t) = int("time") {
        sub.push(ago(now - t));
    }
    let link = match (url, int("id")) {
        (Some(url), _) => url.to_string(),
        (None, Some(id)) => format!("https://news.ycombinator.com/item?id={id}"),
        (None, None) => "https://news.ycombinator.com/".to_string(),
    };
    Some(ListItem {
        tone: None,
        title: title.to_string(),
        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
        status: None,
        tags: url.and_then(domain).into_iter().collect(),
        action: Some(Action::OpenUrl { url: link }),
    })
}

/// The tile: the first `limit` items as a list. `now` is in unix seconds.
fn view(items: &[Value], feed: Feed, limit: usize, now: i64) -> View {
    let stories: Vec<ListItem> = items
        .iter()
        .take(limit)
        .filter_map(|item| story(item, now))
        .collect();
    View {
        status: Status::Ok,
        summary: format!("{} {}", stories.len(), feed.label()),
        root: Widget::List {
            items: stories,
            empty_text: Some("no stories".into()),
        },
    }
}

/// "https://www.Example.com:8080/a?b" -> "example.com". Written out by hand
/// (no URL parser) so the plugins can follow the same steps.
fn domain(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("").to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    (!host.is_empty()).then(|| host.to_string())
}

/// Made-up stories, `secs_ago` seconds old, in the API's item shape.
fn demo_items(now: i64) -> Vec<Value> {
    let item = |id: u64,
                title: &str,
                url: Option<&str>,
                score: u32,
                comments: u32,
                by: &str,
                secs_ago: i64| {
        let mut item = serde_json::json!({
            "id": id, "type": "story", "title": title, "by": by, "score": score,
            "descendants": comments, "time": now - secs_ago,
        });
        if let Some(url) = url {
            item["url"] = url.into();
        }
        item
    };
    vec![
        item(
            41_000_001,
            "A tiny terminal dashboard written in Rust",
            Some("https://github.com/example/dash"),
            412,
            128,
            "ferris",
            2 * 3600,
        ),
        item(
            41_000_002,
            "Ask HN: What do you keep on your second monitor?",
            None,
            187,
            243,
            "curious",
            5 * 3600,
        ),
        item(
            41_000_003,
            "How we cut our build times in half",
            Some("https://www.example.com/blog/faster-builds"),
            96,
            31,
            "buildbot",
            40 * 60,
        ),
        item(
            41_000_004,
            "The history of the status bar",
            Some("https://example.org/status-bars"),
            1,
            0,
            "historian",
            50,
        ),
    ]
}

pub fn catalog() -> CatalogEntry {
    let now = dashboard_host::now_ms() / 1000;
    CatalogEntry {
        description: "Top stories from Hacker News (or new, best, Ask HN, Show HN, jobs), with \
                      points, comments and age. Click a story to open it. Public API, no key."
            .into(),
        tags: crate::strings(&["news", "public-api", "benchmark"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["hacker-news.firebaseio.com"]),
        },
        ..crate::entry(
            "hackernews",
            "Hacker News",
            Size::Medium,
            view(&demo_items(now), Feed::Top, 10, now),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdata(name: &str) -> String {
        format!("{}/testdata/hackernews/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn parses_the_recorded_sample() {
        let items = read_sample(&testdata("top.json")).unwrap();
        // 2025-10-27T12:00:00Z. The sample is a year old, so the host test that
        // runs it at the real time only sees whole days, which rarely tick over.
        let now = 1_761_566_400;
        let v = view(&items, Feed::Top, 10, now);
        let Widget::List { items: stories, .. } = &v.root else {
            panic!("not a list: {:?}", v.root);
        };
        // The deleted, dead and null entries are left out.
        assert_eq!(v.summary, "7 top stories");
        assert_eq!(stories.len(), 7);
        assert_eq!(stories[0].title, "Show HN: A dashboard for the terminal");
        assert_eq!(
            stories[0].subtitle.as_deref(),
            Some("312 points · 97 comments · by alice · 2h ago")
        );
        assert_eq!(stories[0].tags, ["github.com"]);
        // Ask HN: no URL, so the HN page.
        assert_eq!(
            stories[1].action,
            Some(Action::OpenUrl {
                url: "https://news.ycombinator.com/item?id=45000002".into()
            })
        );
        assert!(stories[1].tags.is_empty());
        // One point, no comments yet, www. and a port dropped.
        assert_eq!(
            stories[2].subtitle.as_deref(),
            Some("1 point · 0 comments · by bob · just now")
        );
        assert_eq!(stories[2].tags, ["example.com"]);
        // A job: no comment count.
        assert_eq!(
            stories[3].subtitle.as_deref(),
            Some("5 points · by acme · 3d ago")
        );

        let v = view(&items, Feed::Job, 2, now);
        assert_eq!(v.summary, "2 jobs");
    }

    #[test]
    fn reads_domains() {
        assert_eq!(
            domain("https://www.Example.com:8080/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(domain("http://user@host.io#x").as_deref(), Some("host.io"));
        assert_eq!(domain("example.net/path").as_deref(), Some("example.net"));
        assert_eq!(domain("https:///nohost"), None);
    }

    #[tokio::test]
    async fn rejects_too_big_a_limit() {
        let mut m = HackerNews::default();
        let err = m
            .init(serde_json::json!({ "limit": 31 }))
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("limit"));
        m.init(serde_json::json!({ "limit": 30 })).await.unwrap();
    }

    #[test]
    fn rejects_a_bad_sample() {
        let dir = std::env::temp_dir().join(format!("hn-sample-{}", std::process::id()));
        std::fs::write(&dir, "{}").unwrap();
        let err = read_sample(dir.to_str().unwrap()).unwrap_err();
        std::fs::remove_file(&dir).unwrap();
        assert!(err.to_string().contains("expected a JSON array"), "{err}");
        assert!(read_sample("/no/such/file").is_err());
    }

    #[test]
    fn demo_matches_the_saved_view() {
        // The plugin packages' `demo:` is this same view; see
        // crates/config/src/packages.rs.
        let text = std::fs::read_to_string(testdata("demo.view.json")).unwrap();
        let saved: View = serde_json::from_str(&text).unwrap();
        assert_eq!(catalog().demo, saved);
    }
}
