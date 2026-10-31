//! Open tasks from a todo.txt file (todotxt.org): priorities `(A)`,
//! `+project` and `@context` tags, and the common `due:` and `t:`
//! (threshold) extensions.
//!
//! ```yaml
//! modules:
//!   todo-txt:
//!     config:
//!       path: ~/todo.txt
//!       context: work          # only @work tasks
//! ```

use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View};
use dashboard_host::Module;
use jiff::ToSpan;
use jiff::civil::Date;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Task, expand_home, file_name, link, manifest, parse_date, sort_tasks, tasks_view, today,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The todo.txt file. `~/` is expanded.
    path: String,
    /// Only tasks with this `+project` (without the `+`).
    project: Option<String>,
    /// Only tasks with this `@context` (without the `@`).
    context: Option<String>,
    /// Also list tasks whose `t:` threshold date is still ahead.
    show_future: bool,
    /// Rows shown.
    limit: usize,
    /// Makes each task a link: `{path}` is the file name, `{line}` the
    /// task's line.
    link: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: "~/todo.txt".into(),
            project: None,
            context: None,
            show_future: false,
            limit: 20,
            link: None,
        }
    }
}

#[derive(Default)]
pub struct TodoTxt {
    config: Config,
}

#[async_trait]
impl Module for TodoTxt {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest(
            "todo-txt",
            "todo.txt",
            60_000,
            5_000,
            Size::Medium,
            20,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let path = expand_home(&c.path);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let today = today();
        Ok(view(parse(&text, &path, c), today, c))
    }
}

fn view(mut tasks: Vec<(Task, Option<Date>)>, today: Date, c: &Config) -> View {
    if !c.show_future {
        tasks.retain(|(_, threshold)| threshold.is_none_or(|t| t <= today));
    }
    let mut tasks: Vec<Task> = tasks.into_iter().map(|(t, _)| t).collect();
    sort_tasks(&mut tasks);
    tasks_view(&tasks, today, c.limit)
}

/// The open tasks of a todo.txt file, each with its `t:` threshold date.
fn parse(text: &str, file: &Path, c: &Config) -> Vec<(Task, Option<Date>)> {
    let dir = file.parent().unwrap_or(Path::new(""));
    text.lines()
        .enumerate()
        .filter_map(|(n, line)| {
            let (mut t, threshold) = task(line)?;
            if c.project
                .as_ref()
                .is_some_and(|p| !t.tags.contains(&format!("+{p}")))
                || c.context
                    .as_ref()
                    .is_some_and(|x| !t.tags.contains(&format!("@{x}")))
            {
                return None;
            }
            t.source = Some(file_name(file));
            t.action = link(c.link.as_deref(), dir, file, n + 1);
            Some((t, threshold))
        })
        .collect()
}

/// One line: `(A) 2026-10-20 Call Sam +work @phone due:2026-10-28`.
/// Done (`x ...`), hidden (`h:1`) and blank lines give `None`.
fn task(line: &str) -> Option<(Task, Option<Date>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("x ") {
        return None;
    }
    let mut words = line.split_whitespace().peekable();
    // Priority `(A)`: A ranks 0, Z 25, none 26.
    let mut rank = 26;
    if let Some(w) = words.peek()
        && w.len() == 3
        && w.starts_with('(')
        && w.ends_with(')')
        && let Some(c) = w.chars().nth(1).filter(char::is_ascii_uppercase)
    {
        rank = c as i32 - 'A' as i32;
        words.next();
    }
    // The creation date.
    if words
        .peek()
        .is_some_and(|w| parse_date(w).is_some() && w.len() == 10)
    {
        words.next();
    }
    let mut t = Task {
        rank,
        ..Task::default()
    };
    let mut threshold = None;
    let mut title = vec![];
    for w in words {
        if let Some((key, value)) = w.split_once(':')
            // `10:30` is a time, not an extension.
            && key.starts_with(|c: char| c.is_ascii_alphabetic())
            && !value.is_empty()
            && !key.contains('/')
            && !value.starts_with('/')
        {
            match key {
                "due" => t.due = parse_date(value),
                "t" => threshold = parse_date(value),
                "h" if value == "1" => return None,
                // Other `key:value` extensions (rec:, id:...) are dropped.
                _ => {}
            }
            continue;
        }
        if (w.starts_with('+') || w.starts_with('@')) && w.len() > 1 {
            t.tags.push(w.to_string());
        } else {
            title.push(w);
        }
    }
    t.title = title.join(" ");
    Some((t, threshold))
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
        "(A) {created} Call the bank about the card +finance @phone due:{}\n\
         (B) {created} Write the release notes +dashboard @work due:{}\n\
         x {created} {created} Buy milk @errands\n\
         {created} Pick up the dry cleaning @errands due:{}\n\
         (C) Look into standing desks +home\n\
         Plan the winter trip +travel t:{}\n",
        d(-1),
        d(0),
        d(2),
        d(30),
        created = d(-7),
    );
    let c = Config::default();
    let demo = view(parse(&sample, Path::new("todo.txt"), &c), today, &c);
    CatalogEntry {
        description: "Open tasks from a todo.txt file: priorities, +project and @context tags, \
                      due: dates and t: thresholds. Overdue first."
            .into(),
        tags: crate::strings(&["tasks", "todo.txt", "personal"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry("todo-txt", "todo.txt", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{Status, Widget};
    use jiff::civil::date;

    const SAMPLE: &str = include_str!("../testdata/todo-txt/todo.txt");

    fn titles(v: &View) -> Vec<String> {
        let Widget::List { items, .. } = &v.root else {
            panic!()
        };
        items.iter().map(|i| i.title.clone()).collect()
    }

    #[test]
    fn reads_tasks() {
        let tasks = parse(SAMPLE, Path::new("/home/me/todo.txt"), &Config::default());
        assert_eq!(tasks.len(), 6);
        let (bank, _) = &tasks[0];
        assert_eq!(bank.title, "Call the bank about the card");
        assert_eq!(bank.rank, 0);
        assert_eq!(bank.tags, ["+finance", "@phone"]);
        assert_eq!(bank.due, Some(date(2026, 10, 26)));
        assert_eq!(bank.source.as_deref(), Some("todo.txt"));
        let (_, threshold) = &tasks[5];
        assert_eq!(*threshold, Some(date(2026, 11, 20)));
        // A URL is not a key:value pair.
        assert_eq!(tasks[4].0.title, "Read https://example.com/post");
    }

    #[test]
    fn view_for_a_day() {
        let c = Config::default();
        let v = view(
            parse(SAMPLE, Path::new("todo.txt"), &c),
            date(2026, 10, 27),
            &c,
        );
        assert_eq!(v.summary, "1 overdue · 1 due today · 5 open");
        assert_eq!(v.status, Status::Warn);
        assert_eq!(
            titles(&v),
            [
                "Call the bank about the card",
                "Write the release notes",
                "Pick up the dry cleaning",
                "Look into standing desks",
                "Read https://example.com/post",
            ]
        );

        let c = Config {
            context: Some("work".into()),
            ..Config::default()
        };
        let v = view(
            parse(SAMPLE, Path::new("todo.txt"), &c),
            date(2026, 10, 27),
            &c,
        );
        assert_eq!(titles(&v), ["Write the release notes"]);
    }

    #[test]
    fn lines() {
        assert!(task("x 2026-10-20 done").is_none());
        assert!(task("hidden h:1").is_none());
        assert!(task("   ").is_none());
        let (t, _) = task("(a) lower-case is not a priority").unwrap();
        assert_eq!(t.rank, 26);
        assert_eq!(t.title, "(a) lower-case is not a priority");
        let (t, _) = task("Call Sam at 10:30 due:2026-11-02").unwrap();
        assert_eq!(t.title, "Call Sam at 10:30");
    }
}
