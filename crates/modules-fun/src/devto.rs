//! dev.to: the latest (or top) articles from dev.to, for a tag, an author or
//! the whole site, from the public Forem articles API, as a list. Clicking an
//! article opens it. No account or key.
//!
//! ```yaml
//! modules:
//!   devto:
//!     config: { tag: rust, top: 7, limit: 10 }   # the week's top #rust articles
//! ```

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::{manifest, time::parse_time};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{ago, plural};

const API: &str = "https://dev.to/api/articles";

/// Most articles shown.
const MAX_LIMIT: usize = 30;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Only articles with this tag, e.g. "rust" (no "#").
    tag: Option<String>,
    /// Only articles by this dev.to username.
    username: Option<String>,
    /// The most popular articles of the last this many days, instead of the
    /// latest.
    #[schemars(range(min = 1))]
    top: Option<u32>,
    /// Articles shown, at most 30. Default 10.
    #[schemars(range(min = 1, max = 30))]
    limit: Option<usize>,
    /// Read this file instead of calling the API: a saved `/api/articles`
    /// reply. For tests and demos.
    #[schemars(extend("x-path" = true))]
    sample: Option<String>,
}

impl Config {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(10)
    }

    /// For the summary: "10 top articles · #rust".
    fn label(&self, n: usize) -> String {
        let kind = if self.top.is_some() { "top " } else { "" };
        let noun = if n == 1 { "article" } else { "articles" };
        let mut s = format!("{n} {kind}{noun}");
        if let Some(tag) = &self.tag {
            s.push_str(&format!(" · #{tag}"));
        }
        if let Some(user) = &self.username {
            s.push_str(&format!(" · @{user}"));
        }
        s
    }
}

#[derive(Default)]
pub struct DevTo {
    config: Config,
}

#[async_trait]
impl Module for DevTo {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let c = &self.config;
        if c.limit() == 0 || c.limit() > MAX_LIMIT {
            bail!("limit: 1 to {MAX_LIMIT}");
        }
        if c.top == Some(0) {
            bail!("top: at least 1 day");
        }
        let word = |s: &Option<String>| {
            s.as_deref().is_none_or(|s| {
                !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        };
        if !word(&c.tag) {
            bail!("tag: letters and digits only, without the #");
        }
        if !word(&c.username) {
            bail!("username: letters, digits and _ only");
        }
        Ok(manifest(
            "devto",
            "dev.to",
            600_000,
            60_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let articles: Vec<Article> = match &self.config.sample {
            Some(path) => crate::read_sample(path)?,
            None => fetch(&self.config).await?,
        };
        let now = dashboard_host::now_ms() / 1000;
        Ok(view(&articles, &self.config, now))
    }
}

/// One article from `/api/articles`, with only the fields the tile shows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Article {
    title: String,
    url: String,
    comments_count: i64,
    public_reactions_count: i64,
    published_at: String,
    reading_time_minutes: i64,
    tag_list: Vec<String>,
    user: User,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct User {
    username: String,
}

async fn fetch(c: &Config) -> Result<Vec<Article>> {
    let mut query = vec![("per_page", c.limit().to_string())];
    if let Some(tag) = &c.tag {
        query.push(("tag", tag.clone()));
    }
    if let Some(user) = &c.username {
        query.push(("username", user.clone()));
    }
    if let Some(days) = c.top {
        query.push(("top", days.to_string()));
    }
    let resp = client()?.get(API).query(&query).send().await?;
    Ok(check(resp).await?.json().await?)
}

/// One article as a list item. `now` is in unix seconds.
fn article(a: &Article, now: i64) -> ListItem {
    let mut sub = vec![
        plural(a.public_reactions_count, "reaction"),
        plural(a.comments_count, "comment"),
    ];
    if !a.user.username.is_empty() {
        sub.push(format!("by {}", a.user.username));
    }
    if a.reading_time_minutes > 0 {
        sub.push(format!("{} min read", a.reading_time_minutes));
    }
    if let Some(t) = parse_time(&a.published_at) {
        sub.push(ago(now - t));
    }
    ListItem {
        tone: None,
        title: a.title.clone(),
        subtitle: Some(sub.join(" · ")),
        status: None,
        tags: a.tag_list.iter().take(3).map(|t| format!("#{t}")).collect(),
        action: Some(Action::OpenUrl {
            url: if a.url.is_empty() {
                "https://dev.to/".into()
            } else {
                a.url.clone()
            },
        }),
    }
}

/// The tile: the articles as a list. `now` is in unix seconds.
fn view(articles: &[Article], c: &Config, now: i64) -> View {
    let items: Vec<ListItem> = articles
        .iter()
        .filter(|a| !a.title.is_empty())
        .take(c.limit())
        .map(|a| article(a, now))
        .collect();
    View {
        status: Status::Ok,
        summary: c.label(items.len()),
        root: Widget::List {
            items,
            empty_text: Some("no articles".into()),
        },
    }
}

/// "Now" for the sample: 2026-10-27T12:00:00Z.
const SAMPLE_NOW: i64 = 1_793_102_400;

pub fn catalog() -> CatalogEntry {
    let articles: Vec<Article> =
        serde_json::from_str(include_str!("../testdata/devto/articles.json"))
            .expect("demo data parses");
    let demo = Config {
        tag: Some("rust".into()),
        ..Config::default()
    };
    CatalogEntry {
        description: "The latest or top dev.to articles for a tag, an author or the whole \
                      site, with reactions, comments and reading time. Click an article to \
                      open it. Public API, no key."
            .into(),
        tags: crate::strings(&["news", "dev", "public-api"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["dev.to"]),
        },
        ..crate::entry(
            "devto",
            "dev.to",
            Size::Medium,
            view(&articles, &demo, SAMPLE_NOW),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Article> {
        let path = format!(
            "{}/testdata/devto/articles.json",
            env!("CARGO_MANIFEST_DIR")
        );
        crate::read_sample(&path).unwrap()
    }

    #[test]
    fn parses_the_sample() {
        let c = Config {
            tag: Some("rust".into()),
            top: Some(7),
            ..Config::default()
        };
        let v = view(&sample(), &c, SAMPLE_NOW);
        let Widget::List { items, .. } = &v.root else {
            panic!("not a list: {:?}", v.root);
        };
        assert_eq!(v.summary, "3 top articles · #rust");
        assert_eq!(items[0].title, "Building a terminal dashboard in a weekend");
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("238 reactions · 14 comments · by ferris · 7 min read · 2d ago")
        );
        // At most three tags.
        assert_eq!(items[0].tags, ["#rust", "#tui", "#showdev"]);
        assert_eq!(
            items[0].action,
            Some(Action::OpenUrl {
                url: "https://dev.to/ferris/building-a-terminal-dashboard-in-a-weekend-1a2b".into()
            })
        );
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("1 reaction · 1 comment · by errata · 4 min read · 45m ago")
        );
        assert!(items[2].tags.is_empty());
    }

    #[test]
    fn labels_and_limits() {
        let c = Config {
            username: Some("ferris".into()),
            limit: Some(1),
            ..Config::default()
        };
        let v = view(&sample(), &c, SAMPLE_NOW);
        assert_eq!(v.summary, "1 article · @ferris");
    }

    #[tokio::test]
    async fn checks_the_config() {
        let mut m = DevTo::default();
        for bad in [
            serde_json::json!({ "limit": 31 }),
            serde_json::json!({ "top": 0 }),
            serde_json::json!({ "tag": "#rust" }),
            serde_json::json!({ "username": "" }),
        ] {
            assert!(m.init(bad.clone()).await.is_err(), "{bad}");
        }
        m.init(serde_json::json!({ "tag": "rust", "top": 7 }))
            .await
            .unwrap();
    }
}
