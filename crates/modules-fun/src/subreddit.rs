//! Subreddit: the hot (or new, top, rising) posts of a subreddit, from
//! Reddit's public JSON listing, as a list. Clicking a post opens its Reddit
//! page. No account or key.
//!
//! ```yaml
//! modules:
//!   subreddit:
//!     config: { subreddit: rust, sort: hot, limit: 10 }
//! ```
//!
//! Reddit usually refuses the JSON listing (HTTP 403) to clients without an
//! account. The tile then reads the subreddit's Atom feed (`.rss`) instead,
//! which Reddit still serves. The feed has no points, comments, flair or
//! pinned flag, so those posts show only author and age, and pinned posts
//! stay in. On 429 (too many requests) the tile shows that error until a
//! later refresh gets through.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, ValueFormat, View,
    Widget, format_value,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{Client, check, client};
use dashboard_modules_common::manifest;
use dashboard_modules_common::time::parse_time;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{ago, plural};

const SITE: &str = "https://www.reddit.com";

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Sort {
    Hot,
    New,
    Top,
    Rising,
}

impl Sort {
    fn path(self) -> &'static str {
        match self {
            Sort::Hot => "hot",
            Sort::New => "new",
            Sort::Top => "top",
            Sort::Rising => "rising",
        }
    }
}

/// Most posts shown.
const MAX_LIMIT: usize = 25;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The subreddit's name, without "r/". Join several with "+", e.g.
    /// "rust+programming".
    subreddit: String,
    sort: Sort,
    /// Posts shown, at most 25.
    #[schemars(range(min = 1, max = 25))]
    limit: usize,
    /// Leave out the posts the moderators pinned to the top.
    hide_stickied: bool,
    /// Read this file instead of calling Reddit: a saved
    /// `/r/<name>/<sort>.json` reply, or a saved `/r/<name>/<sort>/.rss` feed
    /// if the name ends in .rss or .xml. For tests and demos.
    #[schemars(extend("x-path" = true))]
    sample: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            subreddit: "rust".into(),
            sort: Sort::Hot,
            limit: 10,
            hide_stickied: true,
            sample: None,
        }
    }
}

#[derive(Default)]
pub struct Subreddit {
    config: Config,
}

#[async_trait]
impl Module for Subreddit {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let c = &self.config;
        if c.limit == 0 || c.limit > MAX_LIMIT {
            bail!("limit: 1 to {MAX_LIMIT}");
        }
        let name_ok = c.subreddit.split('+').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        });
        if !name_ok {
            bail!("subreddit: letters, digits and _ only, e.g. \"rust\" (join several with +)");
        }
        Ok(manifest(
            "subreddit",
            "Subreddit",
            300_000,
            60_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let listing: Listing = match &c.sample {
            Some(path) if path.ends_with(".rss") || path.ends_with(".xml") => {
                let path = dashboard_modules_common::expand_home(path);
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                parse_feed(&text).with_context(|| format!("parsing {}", path.display()))?
            }
            Some(path) => crate::read_sample(path)?,
            None => fetch(&c.subreddit, c.sort, c.limit).await?,
        };
        let now = dashboard_host::now_ms() / 1000;
        Ok(view(
            &listing,
            &c.subreddit,
            c.sort,
            c.limit,
            c.hide_stickied,
            now,
        ))
    }
}

/// A `/r/<name>/<sort>.json` reply, with only the fields the tile shows.
#[derive(Debug, Deserialize)]
struct Listing {
    data: ListingData,
}

#[derive(Debug, Deserialize)]
struct ListingData {
    children: Vec<Child>,
}

#[derive(Debug, Deserialize)]
struct Child {
    data: Post,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Post {
    title: String,
    author: String,
    /// None when read from the Atom feed, which has no counts.
    score: Option<i64>,
    num_comments: Option<i64>,
    created_utc: f64,
    permalink: String,
    domain: String,
    is_self: bool,
    stickied: bool,
    over_18: bool,
    link_flair_text: Option<String>,
}

async fn fetch(subreddit: &str, sort: Sort, limit: usize) -> Result<Listing> {
    // Ask for a few extra, since pinned posts are left out.
    let url = format!(
        "{SITE}/r/{subreddit}/{}.json?limit={}&raw_json=1",
        sort.path(),
        limit + 3
    );
    let http = client()?;
    let resp = http.get(&url).send().await?;
    match resp.status().as_u16() {
        // Reddit refuses the JSON to most clients without an account, but
        // still serves the Atom feed.
        403 => return fetch_feed(&http, subreddit, sort, limit).await,
        429 => bail!("Reddit says too many requests (HTTP 429); try a longer refresh"),
        404 => bail!("no subreddit r/{subreddit}"),
        _ => {}
    }
    Ok(check(resp).await?.json().await?)
}

/// The subreddit's Atom feed, `/r/<name>/<sort>/.rss`, as a listing.
async fn fetch_feed(http: &Client, subreddit: &str, sort: Sort, limit: usize) -> Result<Listing> {
    let url = format!("{SITE}/r/{subreddit}/{}/.rss?limit={limit}", sort.path());
    let resp = http.get(&url).send().await?;
    match resp.status().as_u16() {
        403 | 429 => bail!(
            "Reddit refused both its JSON and its feed (HTTP {}); try a longer refresh",
            resp.status().as_u16()
        ),
        404 => bail!("no subreddit r/{subreddit}"),
        _ => {}
    }
    parse_feed(&check(resp).await?.text().await?)
}

const ATOM_NS: &str = "http://www.w3.org/2005/Atom";

/// A subreddit's Atom feed as a listing. Only title, author, link and time
/// are filled in; the feed has nothing else.
fn parse_feed(text: &str) -> Result<Listing> {
    let doc = roxmltree::Document::parse(text)?;
    let feed = doc.root_element();
    if !feed.has_tag_name((ATOM_NS, "feed")) {
        bail!("not an Atom feed");
    }
    let child = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.children()
            .find(|c| c.has_tag_name((ATOM_NS, name)))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
    let children = feed
        .children()
        .filter(|c| c.has_tag_name((ATOM_NS, "entry")))
        .map(|entry| {
            let link = entry
                .children()
                .find(|c| c.has_tag_name((ATOM_NS, "link")))
                .and_then(|c| c.attribute("href"))
                .unwrap_or_default();
            let author = entry
                .children()
                .find(|c| c.has_tag_name((ATOM_NS, "author")))
                .map(|a| child(a, "name"))
                .unwrap_or_default();
            Child {
                data: Post {
                    title: child(entry, "title"),
                    author: author.trim_start_matches("/u/").to_string(),
                    created_utc: parse_time(&child(entry, "published"))
                        .or_else(|| parse_time(&child(entry, "updated")))
                        .unwrap_or(0) as f64,
                    permalink: link.strip_prefix(SITE).unwrap_or(link).to_string(),
                    is_self: true,
                    ..Post::default()
                },
            }
        })
        .collect();
    Ok(Listing {
        data: ListingData { children },
    })
}

/// One post as a list item. `now` is in unix seconds.
fn post(p: &Post, now: i64) -> ListItem {
    let mut sub = vec![];
    if let Some(score) = p.score {
        sub.push(format!(
            "{} {}",
            format_value(score as f64, ValueFormat::Count, None),
            if score == 1 { "point" } else { "points" }
        ));
    }
    if let Some(n) = p.num_comments {
        sub.push(plural(n, "comment"));
    }
    if !p.author.is_empty() && p.author != "[deleted]" {
        sub.push(format!("u/{}", p.author));
    }
    if p.created_utc > 0.0 {
        sub.push(ago(now - p.created_utc as i64));
    }
    let mut tags = vec![];
    if let Some(flair) = p.link_flair_text.as_deref().map(str::trim)
        && !flair.is_empty()
    {
        tags.push(flair.to_string());
    }
    if !p.is_self && !p.domain.is_empty() {
        tags.push(p.domain.trim_start_matches("www.").to_string());
    }
    if p.over_18 {
        tags.push("nsfw".into());
    }
    ListItem {
        tone: None,
        title: p.title.clone(),
        subtitle: Some(sub.join(" · ")),
        status: None,
        tags,
        action: Some(Action::OpenUrl {
            url: if p.permalink.starts_with("http") {
                p.permalink.clone()
            } else {
                format!("{SITE}{}", p.permalink)
            },
        }),
    }
}

/// The tile: the first `limit` posts as a list. `now` is in unix seconds.
fn view(
    listing: &Listing,
    subreddit: &str,
    sort: Sort,
    limit: usize,
    hide_stickied: bool,
    now: i64,
) -> View {
    let posts: Vec<ListItem> = listing
        .data
        .children
        .iter()
        .map(|c| &c.data)
        .filter(|p| !(p.title.is_empty() || hide_stickied && p.stickied))
        .take(limit)
        .map(|p| post(p, now))
        .collect();
    View {
        status: Status::Ok,
        summary: format!(
            "r/{subreddit} · {} {} {}",
            posts.len(),
            sort.path(),
            if posts.len() == 1 { "post" } else { "posts" }
        ),
        root: Widget::List {
            items: posts,
            empty_text: Some("no posts".into()),
        },
    }
}

/// "Now" for the sample: 2026-10-27T12:00:00Z.
const SAMPLE_NOW: i64 = 1_793_102_400;

pub fn catalog() -> CatalogEntry {
    let listing: Listing = serde_json::from_str(include_str!("../testdata/subreddit/hot.json"))
        .expect("demo data parses");
    CatalogEntry {
        description: "The hot, new, top or rising posts of a subreddit, with points, comments, \
                      author and flair. Click a post to open it on Reddit. No key. Reddit usually \
                      refuses its JSON to clients without an account; the tile then reads the \
                      subreddit's feed, which has only title, author and age."
            .into(),
        tags: crate::strings(&["news", "social", "public-api"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["www.reddit.com"]),
        },
        ..crate::entry(
            "subreddit",
            "Subreddit",
            Size::Medium,
            view(&listing, "rust", Sort::Hot, 10, true, SAMPLE_NOW),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Listing {
        let path = format!("{}/testdata/subreddit/hot.json", env!("CARGO_MANIFEST_DIR"));
        crate::read_sample(&path).unwrap()
    }

    #[test]
    fn parses_the_sample() {
        let v = view(&sample(), "rust", Sort::Hot, 10, true, SAMPLE_NOW);
        let Widget::List { items, .. } = &v.root else {
            panic!("not a list: {:?}", v.root);
        };
        // The pinned post is left out.
        assert_eq!(v.summary, "r/rust · 4 hot posts");
        assert_eq!(items[0].title, "Announcing a faster terminal UI crate");
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("1.5k points · 212 comments · u/ferris_dev · 2h ago")
        );
        assert_eq!(items[0].tags, ["🛠️ project", "github.com"]);
        assert_eq!(
            items[0].action,
            Some(Action::OpenUrl {
                url: "https://www.reddit.com/r/rust/comments/1ab0002/announcing_a_faster_terminal_ui_crate/".into()
            })
        );
        // A text post: no domain tag. One comment.
        assert_eq!(items[1].tags, ["🙋 seeking help & advice"]);
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("87 points · 1 comment · u/new_to_async · 30m ago")
        );
        // An empty flair is left out; one point.
        assert_eq!(items[2].tags, ["lwn.net"]);
        assert!(
            items[2]
                .subtitle
                .as_deref()
                .unwrap()
                .starts_with("1 point · ")
        );
        // A deleted author is left out; nsfw is a tag.
        assert_eq!(
            items[3].subtitle.as_deref(),
            Some("402 points · 96 comments · 2d ago")
        );
        assert_eq!(
            items[3].tags,
            ["📡 official blog", "blog.example.com", "nsfw"]
        );
    }

    #[test]
    fn keeps_pinned_posts_when_asked_and_limits() {
        let v = view(&sample(), "rust", Sort::New, 2, false, SAMPLE_NOW);
        let Widget::List { items, .. } = &v.root else {
            panic!()
        };
        assert_eq!(v.summary, "r/rust · 2 new posts");
        assert_eq!(items[0].title, "Weekly questions thread: ask anything");
    }

    #[test]
    fn reads_the_atom_feed() {
        let path = format!("{}/testdata/subreddit/hot.rss", env!("CARGO_MANIFEST_DIR"));
        let listing = parse_feed(&std::fs::read_to_string(path).unwrap()).unwrap();
        let v = view(&listing, "rust", Sort::Hot, 10, true, SAMPLE_NOW);
        let Widget::List { items, .. } = &v.root else {
            panic!("not a list: {:?}", v.root);
        };
        assert_eq!(v.summary, "r/rust · 2 hot posts");
        assert_eq!(items[0].title, "Announcing a faster terminal UI crate");
        // No points or comments in the feed; "/u/" is dropped.
        assert_eq!(items[0].subtitle.as_deref(), Some("u/ferris_dev · 2h ago"));
        assert!(items[0].tags.is_empty());
        assert_eq!(
            items[0].action,
            Some(Action::OpenUrl {
                url: "https://www.reddit.com/r/rust/comments/1ab0002/announcing_a_faster_terminal_ui_crate/".into()
            })
        );
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("u/new_to_async · 30m ago")
        );
        assert!(parse_feed("<rss/>").is_err());
    }

    #[tokio::test]
    async fn checks_the_config() {
        let mut m = Subreddit::default();
        for bad in [
            serde_json::json!({ "limit": 26 }),
            serde_json::json!({ "limit": 0 }),
            serde_json::json!({ "subreddit": "r/rust" }),
            serde_json::json!({ "subreddit": "rust+" }),
        ] {
            assert!(m.init(bad.clone()).await.is_err(), "{bad}");
        }
        m.init(serde_json::json!({ "subreddit": "rust+programming", "sort": "top" }))
            .await
            .unwrap();
    }
}
