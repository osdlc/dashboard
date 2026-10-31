//! An org-mode agenda: open TODO headlines from `.org` files with their
//! SCHEDULED and DEADLINE dates, overdue first.
//!
//! ```yaml
//! modules:
//!   org-agenda:
//!     config:
//!       paths: [~/org]          # files or folders (searched for .org files)
//!       days: 7                 # look this many days ahead
//!       undated: false          # also list TODOs with no date
//!       link: "https://github.com/me/org/blob/main/{path}#L{line}"
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
    /// `.org` files, or folders to search for them. `~/` is expanded.
    paths: Vec<String>,
    /// Show dated items up to this many days ahead. Overdue ones always show.
    days: i64,
    /// Also list open TODOs that have no date, after the dated ones.
    undated: bool,
    /// Rows shown.
    limit: usize,
    /// Makes each item a link: `{path}` is the file's path under the folder
    /// in `paths`, `{line}` the headline's line. E.g.
    /// `https://github.com/me/org/blob/main/{path}#L{line}`.
    link: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paths: vec![],
            days: 7,
            undated: false,
            limit: 20,
            link: None,
        }
    }
}

#[derive(Default)]
pub struct OrgAgenda {
    config: Config,
}

#[async_trait]
impl Module for OrgAgenda {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        if self.config.paths.is_empty() {
            bail!("the org-agenda module needs `config: {{ paths: [~/org] }}`");
        }
        Ok(manifest(
            "org-agenda",
            "Agenda",
            60_000,
            5_000,
            Size::Medium,
            20,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let mut tasks = vec![];
        for (file, root) in find_files(&c.paths, &["org"])? {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            tasks.extend(parse(&text, &file, &root, c.link.as_deref()));
        }
        Ok(agenda(tasks, today(), c))
    }
}

/// The items to show on `today`, sorted.
fn agenda(tasks: Vec<Task>, today: Date, c: &Config) -> View {
    let until = today.checked_add(c.days.max(0).days()).unwrap_or(today);
    let mut tasks: Vec<Task> = tasks
        .into_iter()
        .filter(|t| t.when().map_or(c.undated, |w| w <= until))
        .collect();
    sort_tasks(&mut tasks);
    tasks_view(&tasks, today, c.limit)
}

/// The TODO keywords of a file: its `#+TODO:` lines, else TODO/DONE plus
/// the common NEXT, WAITING and CANCELLED. Returns (open, done).
fn keywords(text: &str) -> (Vec<String>, Vec<String>) {
    let mut open = vec![];
    let mut done = vec![];
    for line in text.lines() {
        // Org reads `#+todo:` in any case.
        let Some(rest) = ["#+TODO:", "#+SEQ_TODO:", "#+TYP_TODO:"]
            .iter()
            .find_map(|k| {
                line.get(..k.len())
                    .filter(|head| head.eq_ignore_ascii_case(k))
                    .map(|_| &line[k.len()..])
            })
        else {
            continue;
        };
        let mut after_bar = false;
        let words: Vec<&str> = rest.split_whitespace().collect();
        let has_bar = words.contains(&"|");
        for (i, w) in words.iter().enumerate() {
            if *w == "|" {
                after_bar = true;
                continue;
            }
            // `TODO(t)` and `WAIT(w@/!)`: the key in parentheses is dropped.
            let w = w.split('(').next().unwrap_or(w).to_string();
            // With no `|`, the last word is the done state.
            if after_bar || (!has_bar && i == words.len() - 1) {
                done.push(w);
            } else {
                open.push(w);
            }
        }
    }
    if open.is_empty() {
        open = ["TODO", "NEXT", "WAITING"].map(String::from).to_vec();
        done = ["DONE", "CANCELLED", "CANCELED"].map(String::from).to_vec();
    }
    (open, done)
}

/// The open TODO headlines of one file. `root` and `template` make the
/// links (see `link` in lib.rs).
fn parse(text: &str, file: &Path, root: &Path, template: Option<&str>) -> Vec<Task> {
    let (open, done) = keywords(text);
    let mut tasks = vec![];
    // The headline being read, and whether it is open.
    let mut current: Option<(Task, bool)> = None;
    // The line right after a headline, when it starts with DEADLINE:,
    // SCHEDULED: or CLOSED:, is its planning; the words in a note don't count.
    let mut planning = false;
    for (n, line) in text.lines().enumerate() {
        let is_planning = std::mem::take(&mut planning)
            && ["DEADLINE:", "SCHEDULED:", "CLOSED:"]
                .iter()
                .any(|k| line.trim_start().starts_with(k));
        if let Some(head) = headline(line) {
            if let Some((t, true)) = current.take() {
                tasks.push(t);
            }
            let (word, rest) = head.split_once(' ').unwrap_or((head, ""));
            let is_open = open.iter().any(|k| k == word);
            if !is_open && !done.iter().any(|k| k == word) {
                // Not a TODO item.
                current = None;
                continue;
            }
            let (title, rank, tags) = title_parts(rest);
            let mut t = Task {
                title,
                rank,
                tags,
                source: Some(file_name(file)),
                action: link(template, root, file, n + 1),
                ..Task::default()
            };
            if word != "TODO" {
                t.tags.insert(0, word.to_lowercase());
            }
            current = Some((t, is_open));
            planning = true;
            continue;
        }
        if is_planning && let Some((t, _)) = current.as_mut() {
            if let Some(d) = stamp(line, "DEADLINE:") {
                t.due = Some(d);
            }
            if let Some(d) = stamp(line, "SCHEDULED:") {
                t.scheduled = Some(d);
            }
        }
    }
    if let Some((t, true)) = current {
        tasks.push(t);
    }
    tasks
}

/// The text after the stars of a headline (`** TODO Call Sam`).
fn headline(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('*');
    (rest.len() < line.len() && rest.starts_with(' ')).then(|| rest.trim())
}

/// Title, rank from `[#A]` (A = 0, none = B = 1, C = 2) and the `:tags:` at
/// the end.
fn title_parts(rest: &str) -> (String, i32, Vec<String>) {
    let mut rest = rest.trim();
    let mut rank = 1;
    if let Some(p) = rest.strip_prefix("[#")
        && let Some((letter, after)) = p.split_once(']')
        && let Some(c) = letter.chars().next()
    {
        rank = (c.to_ascii_uppercase() as i32) - ('A' as i32);
        rest = after.trim_start();
    }
    let mut tags = vec![];
    if let Some((before, last)) = rest.rsplit_once(char::is_whitespace)
        && last.len() > 2
        && last.starts_with(':')
        && last.ends_with(':')
    {
        tags = last
            .trim_matches(':')
            .split(':')
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect();
        rest = before.trim_end();
    }
    (rest.to_string(), rank, tags)
}

/// The date of `<2026-10-30 Fri>` after `key` on a planning line.
fn stamp(line: &str, key: &str) -> Option<Date> {
    let at = line.find(key)?;
    let rest = line[at + key.len()..].trim_start();
    parse_date(rest.strip_prefix('<')?)
}

pub fn catalog() -> CatalogEntry {
    let today = today();
    let d = |days: i64| {
        today
            .checked_add(days.days())
            .unwrap_or(today)
            .strftime("%Y-%m-%d %a")
            .to_string()
    };
    let sample = format!(
        "#+TODO: TODO NEXT WAITING | DONE CANCELLED\n\
         * Work\n\
         ** TODO [#A] Send the Q3 report :work:\n   DEADLINE: <{}>\n\
         ** NEXT Review Sam's design doc\n   SCHEDULED: <{}>\n\
         ** WAITING Contract back from legal\n   SCHEDULED: <{}> DEADLINE: <{}>\n\
         ** DONE Book the venue\n   CLOSED: [{}]\n\
         * Home\n\
         ** TODO Renew passport :errand:\n   DEADLINE: <{} +1y>\n\
         ** TODO Call the plumber\n   SCHEDULED: <{}>\n",
        d(-2),
        d(0),
        d(-1),
        d(3),
        d(-1),
        d(5),
        d(1),
    );
    let tasks = parse(&sample, Path::new("work.org"), Path::new(""), None);
    let demo = agenda(tasks, today, &Config::default());
    CatalogEntry {
        description: "An org-mode agenda: open TODO headlines from .org files with their \
                      SCHEDULED and DEADLINE dates, overdue first. Reads #+TODO keywords \
                      and [#A] priorities."
            .into(),
        tags: crate::strings(&["tasks", "org-mode", "notes", "personal"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements::default(),
        ..crate::entry("org-agenda", "Agenda", Size::Medium, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{Action, Status, Widget};
    use jiff::civil::date;

    const SAMPLE: &str = include_str!("../testdata/org/agenda.org");

    fn tasks() -> Vec<Task> {
        parse(
            SAMPLE,
            Path::new("/notes/org/agenda.org"),
            Path::new("/notes"),
            Some("https://example.com/{path}#L{line}"),
        )
    }

    #[test]
    fn reads_todo_headlines() {
        let tasks = tasks();
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Send the Q3 report",
                "Review the design doc",
                "Contract back from legal",
                "Renew passport",
                "Someday: learn the cello",
            ]
        );
        let report = &tasks[0];
        assert_eq!(report.rank, 0);
        assert_eq!(report.tags, ["work", "urgent"]);
        assert_eq!(report.due, Some(date(2026, 10, 25)));
        assert_eq!(
            report.action,
            Some(Action::OpenUrl {
                url: "https://example.com/org/agenda.org#L4".into()
            })
        );
        let legal = &tasks[2];
        assert_eq!(legal.tags, ["waiting"]);
        assert_eq!(legal.scheduled, Some(date(2026, 10, 26)));
        assert_eq!(legal.due, Some(date(2026, 10, 30)));
        // The repeater (+1y) is ignored; the date is kept.
        assert_eq!(tasks[3].due, Some(date(2026, 11, 3)));
    }

    #[test]
    fn planning_line_and_lower_case_keywords() {
        let text = "#+todo: TODO WAIT | DONE\n\
                    * WAIT Call Sam\n\
                    Missed the DEADLINE: <2026-01-01 Thu> last time.\n\
                    * TODO Pay rent\n  DEADLINE: <2026-11-01 Sun>\n";
        let tasks = parse(text, Path::new("/a.org"), Path::new("/"), None);
        assert_eq!(tasks[0].tags, ["wait"]);
        assert_eq!(tasks[0].due, None);
        assert_eq!(tasks[1].due, Some(date(2026, 11, 1)));
    }

    #[test]
    fn agenda_for_a_day() {
        let c = Config::default();
        let view = agenda(tasks(), date(2026, 10, 27), &c);
        assert_eq!(view.summary, "1 overdue · 4 open");
        assert_eq!(view.status, Status::Warn);
        let Widget::List { items, .. } = &view.root else {
            panic!()
        };
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        // Renew passport (3 Nov) is within 7 days; the undated one is left out.
        assert_eq!(
            titles,
            [
                "Send the Q3 report",
                "Contract back from legal",
                "Review the design doc",
                "Renew passport",
            ]
        );
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("overdue 2d · agenda.org")
        );
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("due Fri · scheduled yesterday · agenda.org")
        );

        let c = Config {
            days: 3,
            undated: true,
            ..Config::default()
        };
        let view = agenda(tasks(), date(2026, 10, 27), &c);
        assert_eq!(view.summary, "1 overdue · 4 open");
        let Widget::List { items, .. } = &view.root else {
            panic!()
        };
        assert_eq!(items[3].title, "Someday: learn the cello");
    }

    #[test]
    fn custom_keywords() {
        let (open, done) = keywords("#+TODO: TODO(t) WAIT(w@/!) | DONE(d!) KILL(k)\n");
        assert_eq!(open, ["TODO", "WAIT"]);
        assert_eq!(done, ["DONE", "KILL"]);
        let (open, done) = keywords("#+SEQ_TODO: OPEN FIXED\n");
        assert_eq!(open, ["OPEN"]);
        assert_eq!(done, ["FIXED"]);
    }
}
