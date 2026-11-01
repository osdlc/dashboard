//! Feed reader: the newest items of some RSS and Atom feeds, as one list.
//!
//! ```yaml
//! modules:
//!   feedreader:
//!     config:
//!       feeds:
//!         - https://blog.rust-lang.org/feed.xml
//!         - https://hnrss.org/frontpage
//!       max_items: 10
//! ```
//!
//! Reads RSS 2.0 (and 0.91), RSS 1.0 (RDF) and Atom 1.0. Items from every
//! feed are sorted together, newest first; items without a date go last.
//! Times are shown in UTC. A feed whose DOCTYPE declares entities is refused
//! as not valid XML, so a hostile feed cannot blow up into gigabytes of text.
//!
//! This is one of the native-vs-plugin benchmark widgets: the TypeScript and
//! Python versions in `plugins/ext/feedreader-ts` and
//! `plugins/ext/feedreader-py` follow the same rules and must show the same
//! view for the same input. The shared samples and expected views are in
//! `testdata/feedreader/`.

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::client;
use dashboard_modules_common::{expand_home, manifest};
use roxmltree::{Document, Node, ParsingOptions};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::task::JoinSet;

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const RSS1_NS: &str = "http://purl.org/rss/1.0/";
const ATOM_NS: &str = "http://www.w3.org/2005/Atom";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Feed URLs, RSS or Atom. An entry that does not start with `http://`
    /// or `https://` is read as a file, for tests and demos.
    #[schemars(extend("x-path" = true))]
    feeds: Vec<String>,
    /// How many items to show, newest first, across all feeds.
    max_items: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            feeds: vec![],
            max_items: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Feed {
    /// The feed's own title; empty when it has none.
    title: String,
    items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
struct Item {
    title: String,
    /// Only http(s) links; others would not open.
    link: Option<String>,
    /// Seconds since the epoch.
    time: Option<i64>,
}

/// Why a feed could not be read. The tile shows the reason, which is the same
/// in all three versions, so it leaves out the library's error text.
#[derive(Debug, Clone, PartialEq)]
enum Failure {
    Http(u16),
    Network,
    File,
    Xml,
    NotAFeed,
}

impl Failure {
    fn reason(&self) -> String {
        match self {
            Failure::Http(code) => format!("HTTP {code}"),
            Failure::Network => "network error".into(),
            Failure::File => "could not read the file".into(),
            Failure::Xml => "not valid XML".into(),
            Failure::NotAFeed => "not an RSS or Atom feed".into(),
        }
    }
}

#[derive(Default)]
pub struct FeedReader {
    config: Config,
}

#[async_trait]
impl Module for FeedReader {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        if self.config.feeds.is_empty() {
            bail!("list at least one feed URL under feeds");
        }
        if self.config.max_items == 0 {
            bail!("max_items must be at least 1");
        }
        Ok(manifest(
            "feedreader",
            "Feed reader",
            900_000,
            60_000,
            Size::Medium,
            40,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let http = client()?;
        let mut set = JoinSet::new();
        for (i, entry) in self.config.feeds.iter().cloned().enumerate() {
            let http = http.clone();
            set.spawn(async move {
                let feed = read(&http, &entry).await;
                (i, entry, feed)
            });
        }
        let mut feeds = Vec::new();
        while let Some(done) = set.join_next().await {
            feeds.push(done?);
        }
        feeds.sort_by_key(|(i, _, _)| *i);
        let feeds: Vec<_> = feeds.into_iter().map(|(_, e, f)| (e, f)).collect();
        view(&feeds, self.config.max_items)
    }
}

/// Fetches a URL or reads a file. The body is read as UTF-8 whatever the
/// feed declares.
async fn read(http: &reqwest::Client, entry: &str) -> Result<Feed, Failure> {
    let bytes = if is_http(entry) {
        let resp = http.get(entry).send().await.map_err(|_| Failure::Network)?;
        if !resp.status().is_success() {
            return Err(Failure::Http(resp.status().as_u16()));
        }
        resp.bytes().await.map_err(|_| Failure::Network)?.to_vec()
    } else {
        tokio::fs::read(expand_home(entry))
            .await
            .map_err(|_| Failure::File)?
    };
    parse_feed(&String::from_utf8_lossy(&bytes))
}

fn is_http(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Parses RSS 2.0 / 0.91 (`<rss>`), RSS 1.0 (`<rdf:RDF>`) or Atom
/// (`<feed>`). Elements are matched by namespace and name, so `media:title`
/// or `atom:link` inside an RSS item are not taken for its title or link.
fn parse_feed(text: &str) -> Result<Feed, Failure> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    if has_surrogate_ref(text) {
        return Err(Failure::Xml);
    }
    let doc = Document::parse_with_options(text, options).map_err(|_| Failure::Xml)?;
    let root = doc.root_element();
    if declares_entities(&doc, text) || has_bad_pi(&doc) {
        return Err(Failure::Xml);
    }
    match (
        root.tag_name().namespace().unwrap_or(""),
        root.tag_name().name(),
    ) {
        ("", "rss") => {
            let channel = child(root, "", "channel").ok_or(Failure::NotAFeed)?;
            Ok(rss(channel, children(channel, "", "item"), ""))
        }
        (RDF_NS, "RDF") => {
            let channel = child(root, RSS1_NS, "channel").ok_or(Failure::NotAFeed)?;
            Ok(rss(channel, children(root, RSS1_NS, "item"), RSS1_NS))
        }
        (ATOM_NS, "feed") => Ok(atom(root)),
        _ => Err(Failure::NotAFeed),
    }
}

fn rss<'a, 'i>(channel: Node<'a, 'i>, items: impl Iterator<Item = Node<'a, 'i>>, ns: &str) -> Feed {
    let items = items
        .map(|item| {
            let link = child_text(item, ns, "link")
                .filter(|l| is_http(l))
                .or_else(|| {
                    // An RSS 2.0 guid is a permalink unless it says otherwise.
                    child(item, ns, "guid")
                        .filter(|g| g.attribute("isPermaLink") != Some("false"))
                        .map(text)
                        .filter(|l| is_http(l))
                });
            let time = [
                child_text(item, ns, "pubDate"),
                child_text(item, DC_NS, "date"),
            ]
            .into_iter()
            .flatten()
            .find_map(|d| parse_date(&d));
            Item {
                title: child_text(item, ns, "title").unwrap_or_default(),
                link,
                time,
            }
        })
        .collect();
    Feed {
        title: child_text(channel, ns, "title").unwrap_or_default(),
        items,
    }
}

fn atom(feed: Node) -> Feed {
    let items = children(feed, ATOM_NS, "entry")
        .map(|entry| {
            // The first link to a page (rel missing or "alternate") that opens.
            let link = children(entry, ATOM_NS, "link")
                .filter(|l| matches!(l.attribute("rel"), None | Some("alternate")))
                .filter_map(|l| l.attribute("href"))
                .map(|h| h.trim_matches(XML_SPACE).to_string())
                .find(|h| is_http(h));
            let time = [
                child(entry, ATOM_NS, "published").map(text),
                child(entry, ATOM_NS, "updated").map(text),
            ]
            .into_iter()
            .flatten()
            .find_map(|d| parse_date(&d));
            Item {
                title: child(entry, ATOM_NS, "title")
                    .map(atom_text)
                    .unwrap_or_default(),
                link,
                time,
            }
        })
        .collect();
    Feed {
        title: child(feed, ATOM_NS, "title")
            .map(atom_text)
            .unwrap_or_default(),
        items,
    }
}

/// Whether the DOCTYPE declares an entity. All three versions refuse such a
/// feed as not valid XML: Python's parser would expand nested entities with no
/// limit (the "billion laughs"), and the TypeScript parser skips the DTD. The
/// DOCTYPE is what is left of the text before the root once comments and
/// processing instructions are taken out.
fn declares_entities(doc: &Document, text: &str) -> bool {
    let mut prolog = text[..doc.root_element().range().start].to_string();
    for node in doc.root().children().filter(|n| !n.is_element()) {
        let range = node.range();
        if range.end <= prolog.len() {
            prolog.replace_range(range.clone(), &" ".repeat(range.len()));
        }
    }
    prolog.contains("<!ENTITY")
}

/// Whether a processing instruction has a target the other two refuse: `XML`
/// in any case (the real declaration is not a node), or one with a colon.
fn has_bad_pi(doc: &Document) -> bool {
    doc.descendants().any(|n| {
        n.pi()
            .is_some_and(|pi| pi.target.eq_ignore_ascii_case("xml") || pi.target.contains(':'))
    })
}

/// Whether a character reference names a surrogate (`&#xD800;`), which is not
/// an XML character. roxmltree reads it as U+FFFD; the other two refuse it.
/// CDATA sections, comments and processing instructions are skipped: there
/// `&#xD800;` is plain text, not a reference.
fn has_surrogate_ref(text: &str) -> bool {
    let mut rest = text;
    loop {
        let Some(at) = rest.find(['&', '<']) else {
            return false;
        };
        rest = &rest[at..];
        let skip = [("<![CDATA[", "]]>"), ("<!--", "-->"), ("<?", "?>")]
            .into_iter()
            .find(|(open, _)| rest.starts_with(open));
        if let Some((open, close)) = skip {
            match rest[open.len()..].find(close) {
                Some(end) => rest = &rest[open.len() + end + close.len()..],
                None => return false,
            }
            continue;
        }
        if let Some(after) = rest.strip_prefix("&#")
            && let Some((num, _)) = after.split_once(';')
        {
            let n = match num.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16),
                None => num.parse(),
            };
            if n.is_ok_and(|n| (0xD800..=0xDFFF).contains(&n)) {
                return true;
            }
        }
        rest = &rest[1..];
    }
}

fn child<'a, 'i>(node: Node<'a, 'i>, ns: &str, name: &str) -> Option<Node<'a, 'i>> {
    children(node, ns, name).next()
}

fn children<'a, 'i>(
    node: Node<'a, 'i>,
    ns: &str,
    name: &str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    let (ns, name) = (ns.to_string(), name.to_string());
    node.children().filter(move |n| {
        n.is_element()
            && n.tag_name().name() == name
            && n.tag_name().namespace().unwrap_or("") == ns
    })
}

fn child_text(node: Node, ns: &str, name: &str) -> Option<String> {
    child(node, ns, name).map(text)
}

/// All the text inside an element, without tags.
fn raw_text(node: Node) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect()
}

/// The text inside an element, with each run of whitespace made one space.
fn text(node: Node) -> String {
    collapse(&raw_text(node))
}

/// An Atom text: `type="html"` holds escaped HTML, so its tags are dropped
/// and its entities decoded. Plain text and xhtml are read as they are.
fn atom_text(node: Node) -> String {
    if node.attribute("type") == Some("html") {
        collapse(&decode_html(&strip_tags(&raw_text(node))))
    } else {
        text(node)
    }
}

/// XML whitespace. Only these count, in all three versions.
const XML_SPACE: &[char] = &[' ', '\t', '\r', '\n'];

fn collapse(s: &str) -> String {
    s.split(XML_SPACE)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drops every `<...>`. A `<` with no `>` after it stays.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest[start..].find('>') else {
            break;
        };
        out.push_str(&rest[..start]);
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// Decodes `&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`, `&nbsp;` and numeric
/// references. Anything else, or a number that is not a character, stays as
/// it is.
fn decode_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let decoded = rest[1..].find(';').and_then(|end| {
            let name = &rest[1..end + 1];
            let c = match name {
                "lt" => '<',
                "gt" => '>',
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{a0}',
                _ => {
                    let n = match name.strip_prefix('#')? {
                        hex if hex.starts_with(['x', 'X']) => {
                            let digits = &hex[1..];
                            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                                return None;
                            }
                            u32::from_str_radix(digits, 16).ok()?
                        }
                        dec => {
                            if dec.is_empty() || !dec.chars().all(|c| c.is_ascii_digit()) {
                                return None;
                            }
                            dec.parse::<u32>().ok()?
                        }
                    };
                    char::from_u32(n).filter(|&c| c != '\0')?
                }
            };
            Some((c, end + 2))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Seconds since the epoch from an RFC 3339 or RFC 822 date, the two kinds
/// feeds use (often the wrong one for the format). Years before 1970 count
/// as no date: some feeds send `0001-01-01` for "unset".
fn parse_date(s: &str) -> Option<i64> {
    rfc3339(s).or_else(|| rfc822(s)).filter(|&t| t >= 0)
}

/// A cursor over ASCII text, for the two date formats.
struct Scan<'a> {
    s: &'a [u8],
    at: usize,
}

impl Scan<'_> {
    fn eat(&mut self, b: u8) -> bool {
        let hit = self.s.get(self.at) == Some(&b);
        self.at += usize::from(hit);
        hit
    }

    /// A run of `min..=max` ASCII digits, as a number. Takes at most `max`.
    fn digits(&mut self, min: usize, max: usize) -> Option<i64> {
        let start = self.at;
        while self.at - start < max && self.s.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        if self.at - start < min {
            self.at = start;
            return None;
        }
        std::str::from_utf8(&self.s[start..self.at])
            .ok()?
            .parse()
            .ok()
    }

    /// A run of ASCII letters.
    fn letters(&mut self) -> &str {
        let start = self.at;
        while self.s.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
            self.at += 1;
        }
        std::str::from_utf8(&self.s[start..self.at]).unwrap_or("")
    }

    fn done(&self) -> bool {
        self.at == self.s.len()
    }
}

/// `2026-10-27T02:09:57Z`, with optional seconds and fraction, `T`, `t` or a
/// space before the time, and an optional `Z` or `+hh:mm` / `+hhmm` offset
/// (none means UTC). The date alone is midnight UTC. As a regex:
/// `^(\d{4})-(\d{2})-(\d{2})(?:[Tt ](\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?)? ?([Zz]|[+-]\d{2}:?\d{2})?$`
fn rfc3339(s: &str) -> Option<i64> {
    let mut c = Scan {
        s: s.as_bytes(),
        at: 0,
    };
    let y = c.digits(4, 4)?;
    c.eat(b'-').then_some(())?;
    let mo = c.digits(2, 2)?;
    c.eat(b'-').then_some(())?;
    let d = c.digits(2, 2)?;
    let (mut h, mut mi, mut sec) = (0, 0, 0);
    let mark = c.at;
    let time = (|| {
        (c.eat(b'T') || c.eat(b't') || c.eat(b' ')).then_some(())?;
        let h = c.digits(2, 2)?;
        c.eat(b':').then_some(())?;
        let mi = c.digits(2, 2)?;
        let mut sec = 0;
        let before_secs = c.at;
        if c.eat(b':') {
            match c.digits(2, 2) {
                Some(s) => {
                    sec = s;
                    let before_frac = c.at;
                    if c.eat(b'.') && c.digits(1, usize::MAX).is_none() {
                        c.at = before_frac;
                    }
                }
                None => c.at = before_secs,
            }
        }
        Some((h, mi, sec))
    })();
    match time {
        Some(t) => (h, mi, sec) = t,
        None => c.at = mark,
    }
    c.eat(b' ');
    let mut offset = 0;
    if !(c.eat(b'Z') || c.eat(b'z')) {
        let sign = if c.eat(b'+') {
            1
        } else if c.eat(b'-') {
            -1
        } else {
            0
        };
        if sign != 0 {
            let oh = c.digits(2, 2)?;
            c.eat(b':');
            let om = c.digits(2, 2)?;
            offset = sign * (oh * 3600 + om * 60);
        }
    }
    c.done().then_some(())?;
    epoch(y, mo, d, h, mi, sec, offset)
}

/// `Tue, 27 Oct 2026 02:09:57 GMT`: the day name and seconds are optional, a
/// two-digit year below 50 is 20xx, the zone is a name or `+hhmm` (none, or a
/// name we don't know, means UTC). The text has single spaces already. As a
/// regex:
/// `^(?:[A-Za-z]+, ?)?(\d{1,2}) ([A-Za-z]+) (\d{4}|\d{2}) (\d{1,2}):(\d{2})(?::(\d{2}))?(?: ?([A-Za-z]+|[+-]\d{4}))?$`
fn rfc822(s: &str) -> Option<i64> {
    let mut c = Scan {
        s: s.as_bytes(),
        at: 0,
    };
    if !c.letters().is_empty() {
        c.eat(b',').then_some(())?;
        c.eat(b' ');
    } else {
        c.at = 0;
    }
    let d = c.digits(1, 2)?;
    c.eat(b' ').then_some(())?;
    let month = c.letters().to_ascii_lowercase();
    let mo = MONTHS
        .iter()
        .position(|m| month.len() >= 3 && month.starts_with(m))? as i64
        + 1;
    c.eat(b' ').then_some(())?;
    let start = c.at;
    let y = c.digits(2, 4)?;
    let y = match c.at - start {
        4 => y,
        2 if y < 50 => 2000 + y,
        2 => 1900 + y,
        _ => return None,
    };
    c.eat(b' ').then_some(())?;
    let h = c.digits(1, 2)?;
    c.eat(b':').then_some(())?;
    let mi = c.digits(2, 2)?;
    let mut sec = 0;
    if c.eat(b':') {
        sec = c.digits(2, 2)?;
    }
    let mut offset = 0;
    if !c.done() {
        c.eat(b' ');
        if c.eat(b'+') || c.eat(b'-') {
            let sign = if c.s[c.at - 1] == b'+' { 1 } else { -1 };
            let hhmm = c.digits(4, 4)?;
            offset = sign * (hhmm / 100 * 3600 + hhmm % 100 * 60);
        } else {
            let zone = c.letters().to_ascii_uppercase();
            if zone.is_empty() {
                return None;
            }
            offset = ZONES
                .iter()
                .find(|(name, _)| *name == zone)
                .map_or(0, |(_, hours)| hours * 3600);
        }
    }
    c.done().then_some(())?;
    epoch(y, mo, d, h, mi, sec, offset)
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// The US zone names RFC 822 allows. GMT, UT, UTC, Z and any other name are
/// UTC.
const ZONES: [(&str, i64); 8] = [
    ("EST", -5),
    ("EDT", -4),
    ("CST", -6),
    ("CDT", -5),
    ("MST", -7),
    ("MDT", -6),
    ("PST", -8),
    ("PDT", -7),
];

/// Seconds since the epoch from a local time and its offset from UTC, or
/// None when a field is out of range.
fn epoch(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64, offset: i64) -> Option<i64> {
    if y < 1970 || !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60
    {
        return None;
    }
    // Days from civil date (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s - offset)
}

/// `2026-10-27 02:09` (UTC).
fn format_time(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs / 60 % 60
    )
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The tile, from each configured feed (as written in the config) and what
/// reading it gave. Feeds that failed come first, then the newest
/// `max_items` items of the rest. The plugin versions build exactly this.
fn view(feeds: &[(String, Result<Feed, Failure>)], max_items: usize) -> Result<View> {
    let failed: Vec<(&String, &Failure)> = feeds
        .iter()
        .filter_map(|(entry, f)| f.as_ref().err().map(|e| (entry, e)))
        .collect();
    if failed.len() == feeds.len()
        && let Some((entry, why)) = failed.first()
    {
        bail!("could not read any feed: {entry}: {}", why.reason());
    }
    let mut items: Vec<(&str, &Item)> = vec![];
    for (entry, feed) in feeds {
        if let Ok(feed) = feed {
            let name = if feed.title.is_empty() {
                entry.as_str()
            } else {
                feed.title.as_str()
            };
            items.extend(feed.items.iter().map(|i| (name, i)));
        }
    }
    // Newest first, undated last; a stable sort keeps feed order for ties.
    items.sort_by_key(|(_, item)| std::cmp::Reverse(item.time));
    items.truncate(max_items);

    let mut list: Vec<ListItem> = failed
        .iter()
        .map(|(entry, why)| ListItem {
            tone: None,
            title: format!("Could not read {entry}"),
            subtitle: Some(why.reason()),
            status: Some(Status::Error),
            tags: vec![],
            action: None,
        })
        .collect();
    list.extend(items.iter().map(|(feed, item)| ListItem {
        tone: None,
        title: if item.title.is_empty() {
            "(no title)".into()
        } else {
            item.title.clone()
        },
        subtitle: Some(match item.time {
            Some(t) => format!("{feed} · {}", format_time(t)),
            None => feed.to_string(),
        }),
        status: None,
        tags: vec![],
        action: item.link.clone().map(|url| Action::OpenUrl { url }),
    }));

    let read = feeds.len() - failed.len();
    let mut summary = format!(
        "{} from {}",
        plural(items.len(), "item"),
        plural(read, "feed")
    );
    if !failed.is_empty() {
        summary.push_str(&format!(" · {} failed", failed.len()));
    }
    Ok(View {
        status: if failed.is_empty() {
            Status::Ok
        } else {
            Status::Warn
        },
        summary,
        root: Widget::List {
            items: list,
            empty_text: Some("No items in these feeds.".into()),
        },
    })
}

pub fn catalog() -> CatalogEntry {
    let feed = parse_feed(include_str!("../testdata/feedreader/demo.xml"));
    let demo = view(&[("https://blog.example.com/feed.xml".into(), feed)], 10)
        .expect("the demo feed parses");
    CatalogEntry {
        description: "The newest items of some RSS and Atom feeds, as one list. Click an item \
                      to open it."
            .into(),
        tags: crate::strings(&["news", "feeds", "rss", "atom", "reading"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry("feedreader", "Feed reader", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Samples shared with the plugin versions' tests in
    /// `crates/host/tests/bench_feedreader.rs`.
    fn dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/feedreader")
    }

    fn data(name: &str) -> String {
        dir().join(name).to_string_lossy().into_owned()
    }

    /// `<name>.view.json`, with `$DIR` replaced by the samples folder (a feed
    /// with no title is named by its path).
    fn expected(name: &str) -> View {
        let path = dir().join(format!("{name}.view.json"));
        let text = std::fs::read_to_string(path).unwrap();
        let dir = serde_json::to_string(&dir()).unwrap();
        let text = text.replace("$DIR", &dir[1..dir.len() - 1]);
        serde_json::from_str(&text).unwrap()
    }

    async fn render(config: serde_json::Value) -> Result<View> {
        let mut m = FeedReader::default();
        m.init(config).await?;
        m.render().await
    }

    /// The feeds of the `mixed` sample: three good ones, then three that fail.
    fn mixed() -> serde_json::Value {
        let feeds: Vec<String> = [
            "rss2.xml",
            "atom.xml",
            "rdf.xml",
            "broken.xml",
            "notafeed.xml",
            "missing.xml",
        ]
        .iter()
        .map(|n| data(n))
        .collect();
        json!({ "feeds": feeds, "max_items": 8 })
    }

    #[tokio::test]
    async fn samples_match_the_expected_views() {
        for name in ["rss2", "atom", "rdf", "rss091", "demo"] {
            let view = render(json!({ "feeds": [data(&format!("{name}.xml"))] }))
                .await
                .unwrap();
            assert_eq!(view, expected(name), "{name}");
        }
        assert_eq!(render(mixed()).await.unwrap(), expected("mixed"));
    }

    #[test]
    fn the_demo_is_the_demo_sample() {
        assert_eq!(catalog().demo, expected("demo"));
    }

    #[tokio::test]
    async fn a_feed_nobody_can_read_is_an_error() {
        for name in ["broken.xml", "notafeed.xml", "missing.xml"] {
            let err = render(json!({ "feeds": [data(name)] })).await.unwrap_err();
            let err = err.to_string();
            assert!(err.starts_with("could not read any feed: "), "{err}");
        }
        let err = render(json!({ "feeds": [data("missing.xml"), data("broken.xml")] }))
            .await
            .unwrap_err();
        let err = err.to_string();
        assert!(
            err.ends_with("missing.xml: could not read the file"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn rejects_bad_config() {
        for bad in [
            json!({}),
            json!({ "feeds": [] }),
            json!({ "feeds": ["x"], "max_items": 0 }),
            json!({ "feeds": ["x"], "maxitems": 3 }),
            json!({ "feeds": "x" }),
        ] {
            assert!(
                FeedReader::default().init(bad.clone()).await.is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn tells_why_a_feed_failed() {
        assert_eq!(parse_feed("<rss><channel></rss>"), Err(Failure::Xml));
        assert_eq!(parse_feed("<rss/>"), Err(Failure::NotAFeed));
        assert_eq!(
            parse_feed("<feed/>"),
            Err(Failure::NotAFeed),
            "no Atom namespace"
        );
        assert_eq!(parse_feed("plain text"), Err(Failure::Xml));
        assert_eq!(
            parse_feed("<a>&nbsp;</a>"),
            Err(Failure::Xml),
            "unknown entity"
        );
        // A byte order mark is fine.
        let feed = parse_feed("\u{feff}<rss><channel><title>t</title></channel></rss>").unwrap();
        assert_eq!(feed.title, "t");
    }

    #[test]
    fn xml_cases_match_the_table() {
        // The same table checks the plugin versions' parsers.
        let text = std::fs::read_to_string(dir().join("xml-cases.json")).unwrap();
        let cases: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
        for case in cases {
            let xml = case["xml"].as_str().unwrap();
            let got = match parse_feed(xml) {
                Ok(_) => "ok".to_string(),
                Err(why) => why.reason(),
            };
            assert_eq!(got, case["result"].as_str().unwrap(), "{xml:?}");
        }
    }

    #[test]
    fn parses_dates() {
        let t = 1_793_066_997; // 2026-10-27T02:09:57Z
        for ok in [
            "2026-10-27T02:09:57Z",
            "2026-10-27t02:09:57z",
            "2026-10-27 02:09:57",
            "2026-10-27T02:09:57.123456Z",
            "2026-10-27T04:09:57+02:00",
            "2026-10-26T21:09:57-0500",
            "2026-10-27T02:09:57 +00:00",
            "Tue, 27 Oct 2026 02:09:57 GMT",
            "Tue,27 Oct 2026 02:09:57 +0000",
            "Tuesday, 27 October 2026 02:09:57 UT",
            "27 Oct 2026 02:09:57",
            "26 Oct 2026 22:09:57 EDT",
            "26 Oct 26 19:09:57 PDT",
            "27 oct 2026 02:09:57 XYZ",
            "27 Oct 2026 03:09:57+0100",
        ] {
            assert_eq!(parse_date(ok), Some(t), "{ok}");
        }
        assert_eq!(parse_date("2026-10-27"), Some(t - 2 * 3600 - 9 * 60 - 57));
        assert_eq!(parse_date("27 Oct 2026 2:09 GMT"), Some(t - 57));
        assert_eq!(parse_date("1 Jan 70 00:00 GMT"), Some(0), "1970");
        assert_eq!(
            parse_date("1 Jan 49 00:00 GMT"),
            Some(2_493_072_000),
            "2049"
        );
        for bad in [
            "",
            "garbage",
            "2026-10-27T02:09:57Q",
            "2026-13-01T00:00:00Z",
            "2026-10-27T24:00:00Z",
            "2026-10-27T02:09:",
            "0001-01-01T00:00:00Z",
            "1970-01-01T00:00:00+01:00",
            "27 Oct 2026",
            "27 Okt 2026 02:09:57 GMT",
            "27 Oct 202 02:09:57 GMT",
            "27 Oct 2026 02:09:57 GMT ",
            "27 Oct 2026 02:09:5",
            "Tue 27 Oct 2026 02:09:57 GMT",
            "\u{661}\u{662} Oct 2026 02:09:57 GMT",
        ] {
            assert_eq!(parse_date(bad), None, "{bad:?}");
        }
        assert_eq!(format_time(t), "2026-10-27 02:09");
        assert_eq!(format_time(0), "1970-01-01 00:00");
    }

    #[test]
    fn cleans_html_titles() {
        assert_eq!(strip_tags("a <b>b</b> c<d"), "a b c<d");
        assert_eq!(
            decode_html("&lt;&gt;&amp;&quot;&apos;&nbsp;&#65;&#x42;&#X43;"),
            "<>&\"'\u{a0}ABC"
        );
        for kept in [
            "&bogus;",
            "&#0;",
            "&#xD800;",
            "&#99999999999;",
            "&#;",
            "&#x;",
            "& amp;",
            "&amp",
        ] {
            assert_eq!(decode_html(kept), kept);
        }
        assert_eq!(decode_html("&amp;amp;"), "&amp;", "one pass");
        assert_eq!(collapse(" a\t\r\n b  \u{a0}c "), "a b \u{a0}c");
    }

    /// Answers `GET /feed` with the rss2 sample and anything else with 404,
    /// for `n` requests.
    async fn serve(n: usize) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = std::fs::read(data("rss2.xml")).unwrap();
        tokio::spawn(async move {
            for _ in 0..n {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 4096];
                let len = sock.read(&mut buf).await.unwrap();
                let found = buf[..len].starts_with(b"GET /feed ");
                let head = match found {
                    true => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len()),
                    false => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n".into(),
                };
                sock.write_all(format!("{head}Connection: close\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                if found {
                    sock.write_all(&body).await.unwrap();
                }
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn fetches_over_http() {
        let base = serve(2).await;
        let feeds = [format!("{base}/feed"), format!("{base}/gone")];
        let view = render(json!({ "feeds": feeds })).await.unwrap();
        assert_eq!(view.status, Status::Warn);
        assert_eq!(view.summary, "7 items from 1 feed · 1 failed");
        let Widget::List { items, .. } = view.root else {
            panic!("not a list");
        };
        assert_eq!(items[0].title, format!("Could not read {base}/gone"));
        assert_eq!(items[0].subtitle.as_deref(), Some("HTTP 404"));
        assert_eq!(items[1].title, "An item with a permalink guid");
    }
}
