//! Open Markdown tasks (`- [ ] ...`) from notes, in the Obsidian Tasks
//! format: 📅 due, ⏳ scheduled and 🛫 start dates, priorities from 🔺 to ⏬,
//! and `#tags`. The Dataview form (`[due:: 2026-10-30]`) works too.
//!
//! ```yaml
//! modules:
//!   markdown-tasks:
//!     config:
//!       paths: [~/Notes]        # files or folders (searched for .md files)
//!       filter: "#task"         # only lines with this text (Obsidian's global filter)
//!       dated_only: false       # only tasks with a date
//!       link: "obsidian://open?vault=Notes&file={path}"
//! ```

use std::path::Path;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View};
use dashboard_host::Module;
use jiff::ToSpan;
use jiff::civil::Date;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Task, file_name, find_files, link, manifest, parse_date, sort_tasks, tasks_view, today,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Markdown files, or folders to search for them. `~/` is expanded.
    paths: Vec<String>,
    /// Only tasks whose line contains this text, e.g. `#task` (the Obsidian
    /// Tasks global filter). It is removed from the title.
    filter: Option<String>,
    /// Only tasks with a due, scheduled or start date.
    dated_only: bool,
    /// Rows shown.
    limit: usize,
    /// Makes each task a link: `{path}` is the file's path under the folder
    /// in `paths`, `{line}` the task's line. E.g.
    /// `obsidian://open?vault=Notes&file={path}`.
    link: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paths: vec![],
            filter: None,
            dated_only: false,
            limit: 20,
            link: None,
        }
    }
}

#[derive(Default)]
pub struct MarkdownTasks {
    config: Config,
}

#[async_trait]
impl Module for MarkdownTasks {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        if self.config.paths.is_empty() {
            bail!("the markdown-tasks module needs `config: {{ paths: [~/Notes] }}`");
        }
        Ok(manifest(
            "markdown-tasks",
            "Tasks",
            60_000,
            5_000,
            Size::Medium,
            20,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let mut tasks = vec![];
        for (file, root) in find_files(&c.paths, &["md", "markdown"])? {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            tasks.extend(parse(&text, &file, &root, c));
        }
        Ok(view(tasks, today(), c))
    }
}

fn view(mut tasks: Vec<Task>, today: Date, c: &Config) -> View {
    if c.dated_only {
        tasks.retain(|t| t.when().is_some());
    }
    sort_tasks(&mut tasks);
    tasks_view(&tasks, today, c.limit)
}

/// Obsidian Tasks priorities, highest first; no emoji ranks between 🔼 and 🔽.
const PRIORITIES: [(&str, i32); 5] = [("🔺", 0), ("⏫", 1), ("🔼", 2), ("🔽", 4), ("⏬", 5)];
const NO_PRIORITY: i32 = 3;

/// The open tasks of one file.
fn parse(text: &str, file: &Path, root: &Path, c: &Config) -> Vec<Task> {
    let mut tasks = vec![];
    let mut in_code = false;
    for (n, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let Some(body) = open_task(trimmed) else {
            continue;
        };
        let mut body = body.to_string();
        if let Some(f) = &c.filter {
            let Some(at) = find_word(&body, f) else {
                continue;
            };
            body.replace_range(at..at + f.len(), "");
        }
        let mut t = task(&body);
        t.source = Some(file_name(file));
        t.action = link(c.link.as_deref(), root, file, n + 1);
        tasks.push(t);
    }
    tasks
}

/// The text after `- [ ] ` (also `*`, `+` and `1.` lists), for open tasks.
/// Done (`[x]`) and cancelled (`[-]`) tasks give `None`.
fn open_task(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| {
            let (num, rest) = line.split_once(". ")?;
            num.chars().all(|c| c.is_ascii_digit()).then_some(rest)
        })?;
    let rest = rest.trim_start();
    // `[ ]` is open, `[/]` in progress; anything else is done or cancelled.
    let rest = rest
        .strip_prefix("[ ]")
        .or_else(|| rest.strip_prefix("[/]"))?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

/// Where `word` is in `text` as a whole word: `#task` but not `#tasks`.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let part = |c: char| c.is_alphanumeric() || "-_/".contains(c);
    text.match_indices(word).map(|(i, _)| i).find(|&i| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(part) && !after.is_some_and(part)
    })
}

/// A task from the text of its line: the dates, priority and tags come out
/// of the title.
fn task(body: &str) -> Task {
    let mut t = Task {
        rank: NO_PRIORITY,
        ..Task::default()
    };
    // Dataview fields: `[due:: 2026-10-30]`.
    let (body, fields) = dataview(body);
    // `📅2026-10-30` with no space works in Obsidian Tasks too.
    let mut body = body.replace("🗓️", "🗓");
    for m in ["📅", "📆", "🗓", "⏳", "⌛", "🛫", "➕", "✅", "❌"] {
        body = body.replace(m, &format!(" {m} "));
    }
    let mut title = vec![];
    let mut words = body.split_whitespace().peekable();
    while let Some(w) = words.next() {
        let date_after = |words: &mut std::iter::Peekable<std::str::SplitWhitespace>| {
            words.peek().and_then(|d| parse_date(d)).inspect(|_| {
                words.next();
            })
        };
        match w {
            "📅" | "📆" | "🗓" | "🗓️" => t.due = date_after(&mut words).or(t.due),
            "⏳" | "⌛" => t.scheduled = date_after(&mut words).or(t.scheduled),
            // A start date: the task can't be done before it, so it counts
            // as scheduled when there is no ⏳.
            "🛫" => {
                if let Some(d) = date_after(&mut words) {
                    t.scheduled = t.scheduled.or(Some(d));
                }
            }
            // Created, done and cancelled dates.
            "➕" | "✅" | "❌" => {
                date_after(&mut words);
            }
            // A repeat rule runs to the next marker; it is dropped.
            "🔁" => {
                while words.peek().is_some_and(|w| !is_marker(w)) {
                    words.next();
                }
            }
            "🆔" | "⛔" | "🏁" => {
                words.next();
            }
            w if PRIORITIES.iter().any(|(p, _)| *p == w) => {
                t.rank = PRIORITIES
                    .iter()
                    .find(|(p, _)| *p == w)
                    .map_or(NO_PRIORITY, |p| p.1);
            }
            w if w.starts_with('#') && w.len() > 1 && !w[1..].starts_with('#') => {
                t.tags.push(w[1..].to_string());
            }
            w => title.push(w),
        }
    }
    for (key, value) in fields {
        let date = parse_date(&value);
        match key.as_str() {
            "due" => t.due = date.or(t.due),
            "scheduled" => t.scheduled = date.or(t.scheduled),
            "start" => t.scheduled = t.scheduled.or(date),
            "priority" => {
                t.rank = match value.as_str() {
                    "highest" => 0,
                    "high" => 1,
                    "medium" => 2,
                    "low" => 4,
                    "lowest" => 5,
                    _ => t.rank,
                }
            }
            _ => {}
        }
    }
    t.title = title.join(" ");
    t
}

/// An Obsidian Tasks emoji, which ends a repeat rule.
fn is_marker(word: &str) -> bool {
    [
        "📅", "📆", "🗓", "🗓️", "⏳", "⌛", "🛫", "➕", "✅", "❌", "🔁", "🆔", "⛔", "🏁",
    ]
    .contains(&word)
        || PRIORITIES.iter().any(|(p, _)| *p == word)
}

/// The `[key:: value]` (and `(key:: value)`) fields in a line, and the line
/// without them.
fn dataview(body: &str) -> (String, Vec<(String, String)>) {
    let mut fields = vec![];
    let mut text = String::new();
    let mut rest = body;
    while let Some(open) = rest.find(['[', '(']) {
        let close_char = if rest[open..].starts_with('[') {
            ']'
        } else {
            ')'
        };
        let after = &rest[open + 1..];
        let field = after.find(close_char).and_then(|close| {
            let (k, v) = after[..close].split_once("::")?;
            let k = k.trim();
            (!k.is_empty() && !k.contains(' '))
                .then(|| (k.to_string(), v.trim().to_string(), close))
        });
        match field {
            Some((k, v, close)) => {
                text.push_str(&rest[..open]);
                fields.push((k, v));
                rest = &after[close + 1..];
            }
            None => {
                text.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    text.push_str(rest);
    (text, fields)
}

pub fn catalog() -> CatalogEntry {
    let today = today();
    let d = |days: i64| {
        today
            .checked_add(days.days())
            .unwrap_or(today)
            .strftime("%Y-%m-%d")
            .to_string()
    };
    let sample = format!(
        "# Inbox\n\n\
         - [ ] Pay the electricity bill ⏫ 📅 {}\n\
         - [ ] Draft the blog post #writing ⏳ {} 📅 {}\n\
         - [x] Water the plants ✅ {}\n\
         - [ ] Book flights for the offsite #travel 📅 {}\n\
         - [ ] Water the plants 🔁 every week 📅 {}\n\
         - [ ] Read \"Designing Data-Intensive Applications\" 🔽\n",
        d(-1),
        d(0),
        d(4),
        d(-1),
        d(0),
        d(6),
    );
    let c = Config::default();
    let tasks = parse(&sample, Path::new("Inbox.md"), Path::new(""), &c);
    let demo = view(tasks, today, &c);
    CatalogEntry {
        description: "Open Markdown tasks (- [ ]) from your notes, in the Obsidian Tasks \
                      format: due, scheduled and start dates, priorities and #tags. Overdue \
                      first."
            .into(),
        tags: crate::strings(&["tasks", "markdown", "obsidian", "notes", "personal"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry("markdown-tasks", "Tasks", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{Action, Status, Widget};
    use jiff::civil::date;

    const SAMPLE: &str = include_str!("../testdata/markdown/Tasks.md");

    fn tasks(c: &Config) -> Vec<Task> {
        parse(
            SAMPLE,
            Path::new("/vault/Projects/Tasks.md"),
            Path::new("/vault"),
            c,
        )
    }

    #[test]
    fn reads_obsidian_tasks() {
        let c = Config {
            link: Some("obsidian://open?vault=Notes&file={path}".into()),
            ..Config::default()
        };
        let tasks = tasks(&c);
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Pay the electricity bill",
                "Draft the blog post",
                "Water the plants",
                "Renew the car insurance",
                "Fix the leaking tap",
                "Plan the offsite",
                "Read the onboarding guide",
            ]
        );
        let bill = &tasks[0];
        assert_eq!(bill.rank, 1);
        assert_eq!(bill.due, Some(date(2026, 10, 26)));
        assert_eq!(bill.tags, ["home"]);
        assert_eq!(
            bill.action,
            Some(Action::OpenUrl {
                url: "obsidian://open?vault=Notes&file=Projects/Tasks.md".into()
            })
        );
        let post = &tasks[1];
        assert_eq!(post.scheduled, Some(date(2026, 10, 27)));
        assert_eq!(post.due, Some(date(2026, 10, 31)));
        assert_eq!(post.rank, 0);
        // The repeat rule is dropped from the title.
        assert_eq!(tasks[2].due, Some(date(2026, 10, 27)));
        // Dataview fields.
        let insurance = &tasks[3];
        assert_eq!(insurance.due, Some(date(2026, 11, 2)));
        assert_eq!(insurance.rank, 1);
        // 🛫 start counts as scheduled.
        assert_eq!(tasks[4].scheduled, Some(date(2026, 10, 29)));
        assert_eq!(tasks[5].rank, NO_PRIORITY);
        assert_eq!(tasks[6].rank, 4);
    }

    #[test]
    fn view_and_filters() {
        let c = Config::default();
        let v = view(tasks(&c), date(2026, 10, 27), &c);
        assert_eq!(v.summary, "1 overdue · 1 due today · 7 open");
        assert_eq!(v.status, Status::Warn);
        let Widget::List { items, .. } = &v.root else {
            panic!()
        };
        assert_eq!(items[0].title, "Pay the electricity bill");
        assert_eq!(items[0].status, Some(Status::Error));
        assert_eq!(items[0].subtitle.as_deref(), Some("overdue 1d · Tasks.md"));
        assert_eq!(items[1].title, "Draft the blog post");
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("due Sat · scheduled today · Tasks.md")
        );
        assert_eq!(items[2].status, Some(Status::Warn));
        assert_eq!(items[6].title, "Read the onboarding guide");

        let c = Config {
            dated_only: true,
            ..Config::default()
        };
        let v = view(tasks(&c), date(2026, 10, 27), &c);
        assert_eq!(v.summary, "1 overdue · 1 due today · 5 open");

        let c = Config {
            filter: Some("#task".into()),
            ..Config::default()
        };
        let titles: Vec<String> = tasks(&c).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Fix the leaking tap", "Plan the offsite"]);
    }

    #[test]
    fn task_lines() {
        assert_eq!(open_task("- [ ] a"), Some("a"));
        assert_eq!(open_task("* [/] a"), Some("a"));
        assert_eq!(open_task("12. [ ] a"), Some("a"));
        assert_eq!(open_task("- [x] a"), None);
        assert_eq!(open_task("- [-] a"), None);
        assert_eq!(open_task("- [ ]"), None);
        assert_eq!(open_task("- a"), None);
        let t = task("Pay rent 📅2026-11-01 ⏳ 2026-10-30");
        assert_eq!(t.title, "Pay rent");
        assert_eq!(t.due, Some(date(2026, 11, 1)));
        assert_eq!(t.scheduled, Some(date(2026, 10, 30)));
        assert_eq!(find_word("a #tasks #task", "#task"), Some(9));
        assert_eq!(find_word("a #tasks", "#task"), None);
        let (text, fields) = dataview("Call [Sam](x) [due::2026-11-02] (priority:: high) now");
        assert_eq!(text, "Call [Sam](x)   now");
        assert_eq!(
            fields,
            [
                ("due".to_string(), "2026-11-02".to_string()),
                ("priority".to_string(), "high".to_string())
            ]
        );
    }
}
