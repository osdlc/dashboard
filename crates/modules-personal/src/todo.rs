//! Open tasks from a local todo file: a markdown checklist or a todo.txt file.
//!
//! ```yaml
//! modules:
//!   todo:
//!     config:
//!       path: ~/notes/todo.md   # .md and .markdown are read as checklists
//!       format: auto            # or markdown, todotxt
//!       show_done: false        # list finished tasks too, after the open ones
//!       limit: 30               # rows shown
//! ```
//!
//! Markdown: `- [ ] task` and `- [x] done`, with `##` headings as tags.
//! todo.txt: `(A)` priorities, `x` for done, `+project` and `@context` tags,
//! and `due:YYYY-MM-DD`. Both formats take `due:` dates; a task due today
//! is a warning and an overdue one an error. The tile itself is only a
//! warning when something is overdue. In todo.txt, any line that is
//! not blank is a task, so a plain list of lines works too.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, ListItem, Manifest, Size, Status, View, Widget};
use dashboard_host::Module;
use dashboard_modules_common::{expand_home, manifest};
use jiff::civil::Date;
use schemars::JsonSchema;
use serde::Deserialize;

/// Larger files are refused: a todo list this big is something else.
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Format {
    /// Markdown for `.md` and `.markdown` files, todo.txt for the rest.
    #[default]
    Auto,
    Markdown,
    Todotxt,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The todo file. `~/` is expanded.
    #[schemars(extend("x-path" = true))]
    path: Option<String>,
    format: Format,
    /// List finished tasks too, after the open ones.
    show_done: bool,
    /// Rows shown.
    limit: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: None,
            format: Format::Auto,
            show_done: false,
            limit: 30,
        }
    }
}

/// One task from the file.
#[derive(Debug, Clone, PartialEq)]
struct Task {
    title: String,
    done: bool,
    /// todo.txt priority letter.
    priority: Option<char>,
    due: Option<Date>,
    /// todo.txt `t:` date: hidden until then.
    threshold: Option<Date>,
    /// `+project`, `@context`, or the markdown section.
    tags: Vec<String>,
}

#[derive(Default)]
pub struct Todo {
    config: Config,
    path: PathBuf,
    name: String,
    markdown: bool,
}

#[async_trait]
impl Module for Todo {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let Some(path) = &self.config.path else {
            bail!("the todo module needs `config: {{ path: <file> }}`");
        };
        if self.config.limit == 0 {
            bail!("`limit` must be at least 1");
        }
        self.path = expand_home(path);
        self.name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Todo".into());
        self.markdown = match self.config.format {
            Format::Markdown => true,
            Format::Todotxt => false,
            Format::Auto => is_markdown(&self.path),
        };
        Ok(manifest("todo", "Todo", 5_000, 1_000, Size::Tall, 0))
    }

    async fn render(&mut self) -> Result<View> {
        let meta = std::fs::metadata(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        if meta.len() > MAX_BYTES {
            bail!("{} is over 1 MB", self.path.display());
        }
        let text = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        let tasks = if self.markdown {
            parse_markdown(&text)
        } else {
            parse_todotxt(&text)
        };
        let today = jiff::Zoned::now().date();
        let c = &self.config;
        Ok(view(&tasks, today, &self.name, c.show_done, c.limit))
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

/// `- [ ] task` and `- [x] task` lines (`*` and `+` bullets too, at any
/// indent). Other lines are skipped; a `##` (or deeper) heading tags the
/// tasks under it.
fn parse_markdown(text: &str) -> Vec<Task> {
    let mut section: Option<String> = None;
    let mut tasks = vec![];
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            let heading = rest.trim_start_matches('#').trim();
            section = (level > 1 && !heading.is_empty()).then(|| heading.to_string());
            continue;
        }
        let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|b| line.strip_prefix(b)) else {
            continue;
        };
        let (done, text) = if let Some(t) = rest.strip_prefix("[ ]") {
            (false, t)
        } else if let Some(t) = rest
            .strip_prefix("[x]")
            .or_else(|| rest.strip_prefix("[X]"))
        {
            (true, t)
        } else {
            continue;
        };
        let mut task = words(text.trim());
        task.done = done;
        if let Some(s) = &section {
            task.tags.insert(0, s.clone());
        }
        if !task.title.is_empty() {
            tasks.push(task);
        }
    }
    tasks
}

/// todo.txt: one task per line. `x ` marks it done, then an optional
/// completion date; `(A) ` is the priority; then an optional creation date.
fn parse_todotxt(text: &str) -> Vec<Task> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let (done, mut rest) = match line.strip_prefix("x ") {
                Some(r) => (true, skip_date(r.trim_start())),
                None => (false, line),
            };
            let mut priority = None;
            let b = rest.as_bytes();
            if b.len() >= 4 && b[0] == b'(' && b[1].is_ascii_uppercase() && &b[2..4] == b") " {
                priority = Some(b[1] as char);
                rest = &rest[4..];
            }
            let mut task = words(skip_date(rest.trim_start()));
            task.done = done;
            task.priority = priority;
            (!task.title.is_empty()).then_some(task)
        })
        .collect()
}

/// Drops a leading `YYYY-MM-DD `.
fn skip_date(s: &str) -> &str {
    match s.split_once(' ') {
        Some((first, rest)) if first.parse::<Date>().is_ok() => rest.trim_start(),
        _ => s,
    }
}

/// Splits the `+project`, `@context`, `due:` and `t:` words out of a task's text.
fn words(text: &str) -> Task {
    let mut task = Task {
        title: String::new(),
        done: false,
        priority: None,
        due: None,
        threshold: None,
        tags: vec![],
    };
    let mut title = vec![];
    for w in text.split_whitespace() {
        let tag = w.len() > 1 && (w.starts_with('+') || w.starts_with('@'));
        if tag {
            task.tags.push(w.to_string());
        } else if let Some(d) = w.strip_prefix("due:").and_then(|d| d.parse().ok()) {
            task.due = Some(d);
        } else if let Some(d) = w.strip_prefix("t:").and_then(|d| d.parse().ok()) {
            task.threshold = Some(d);
        } else {
            title.push(w);
        }
    }
    task.title = title.join(" ");
    task
}

/// "due today", "due tomorrow", "due Nov 15", "overdue 7d".
fn due_text(due: Date, today: Date) -> String {
    let days = (due - today).get_days();
    match days {
        0 => "due today".into(),
        1 => "due tomorrow".into(),
        d if d < 0 => format!("overdue {}d", -d),
        _ => format!("due {}", due.strftime("%b %-d")),
    }
}

/// The tile: open tasks in file order, then (with `show_done`) done ones.
fn view(tasks: &[Task], today: Date, name: &str, show_done: bool, limit: usize) -> View {
    let visible = |t: &&Task| t.threshold.is_none_or(|d| d <= today);
    let open: Vec<&Task> = tasks.iter().filter(|t| !t.done).filter(visible).collect();
    let done = tasks.iter().filter(|t| t.done).count();
    let overdue = open
        .iter()
        .filter(|t| t.due.is_some_and(|d| d < today))
        .count();
    let due_today = open.iter().filter(|t| t.due == Some(today)).count();

    let mut summary = vec![format!("{} open", open.len())];
    if done > 0 {
        summary.push(format!("{done} done"));
    }
    if overdue > 0 {
        summary.push(format!("{overdue} overdue"));
    }
    if due_today > 0 {
        summary.push(format!("{due_today} due today"));
    }

    let shown = open
        .iter()
        .copied()
        .chain(tasks.iter().filter(|t| show_done && t.done));
    let items = shown
        .take(limit)
        .map(|t| {
            let mut tags: Vec<String> = t.priority.map(|p| p.to_string()).into_iter().collect();
            tags.extend(t.tags.iter().cloned());
            if t.done {
                tags.push("done".into());
            }
            let status = match t.due {
                _ if t.done => Some(Status::Ok),
                Some(d) if d < today => Some(Status::Error),
                Some(d) if d == today => Some(Status::Warn),
                _ => None,
            };
            ListItem {
                tone: None,
                title: t.title.clone(),
                subtitle: t.due.filter(|_| !t.done).map(|d| due_text(d, today)),
                status,
                tags,
                action: None,
            }
        })
        .collect();

    View {
        // Warn, not Error: a late task is a nudge, while an error tile means
        // the module itself failed (file missing, unreadable). The overdue
        // rows are still marked as errors.
        status: if overdue > 0 {
            Status::Warn
        } else {
            Status::Ok
        },
        summary: format!("{name} · {}", summary.join(" · ")),
        root: Widget::List {
            items,
            empty_text: Some("Nothing to do.".into()),
        },
    }
}

const SAMPLE_TXT: &str = include_str!("../testdata/todo/todo.txt");

/// The demo's "today", so its due dates read the same whenever it is shown.
fn sample_today() -> Date {
    jiff::civil::date(2026, 10, 27)
}

pub fn catalog() -> CatalogEntry {
    let demo = view(
        &parse_todotxt(SAMPLE_TXT),
        sample_today(),
        "todo.txt",
        false,
        30,
    );
    CatalogEntry {
        description: "Open tasks from a local file: a markdown checklist (`- [ ]`) or \
                      todo.txt, with priorities, tags and due dates. Overdue tasks are \
                      flagged."
            .into(),
        tags: crate::strings(&["todo", "tasks", "personal", "local", "files"]),
        config_schema: crate::schema::<Config>(),
        ..crate::entry("todo", "Todo", Size::Tall, demo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MD: &str = include_str!("../testdata/todo/todo.md");

    fn items(v: &View) -> &[ListItem] {
        let Widget::List { items, .. } = &v.root else {
            panic!("list")
        };
        items
    }

    #[test]
    fn parses_a_markdown_checklist() {
        let tasks = parse_markdown(SAMPLE_MD);
        assert_eq!(tasks.len(), 7);
        assert_eq!(tasks[0].title, "Review the release notes");
        assert_eq!(tasks[0].tags, ["Work"]);
        assert_eq!(tasks[0].due, Some(jiff::civil::date(2026, 10, 27)));
        assert!(tasks[2].done);
        // Nested items count.
        assert_eq!(tasks[4].title, "Ask for the budget");
        assert_eq!(tasks[5].tags, ["Home"]);
        assert!(tasks[6].done);

        let v = view(&tasks, sample_today(), "todo.md", false, 30);
        assert_eq!(v.status, Status::Warn);
        assert_eq!(
            v.summary,
            "todo.md · 5 open · 2 done · 1 overdue · 1 due today"
        );
        let items = items(&v);
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].status, Some(Status::Warn));
        assert_eq!(items[0].subtitle.as_deref(), Some("due today"));
        assert_eq!(items[2].status, Some(Status::Error));
        assert_eq!(items[2].subtitle.as_deref(), Some("overdue 7d"));
        assert_eq!(items[4].subtitle.as_deref(), Some("due Nov 15"));
    }

    #[test]
    fn parses_todotxt() {
        let tasks = parse_todotxt(SAMPLE_TXT);
        assert_eq!(tasks.len(), 6);
        let call = &tasks[0];
        assert_eq!(call.title, "Call the bank about the card");
        assert_eq!(call.priority, Some('A'));
        assert_eq!(call.tags, ["+money", "@phone"]);
        assert_eq!(call.due, Some(jiff::civil::date(2026, 10, 26)));
        let water = &tasks[2];
        assert!(water.done);
        assert_eq!(water.title, "Water the plants");
        assert_eq!(tasks[4].threshold, Some(jiff::civil::date(2026, 11, 1)));
        assert_eq!(tasks[5].title, "Buy coffee beans");

        let v = catalog().demo;
        assert_eq!(
            v.summary,
            "todo.txt · 4 open · 1 done · 1 overdue · 1 due today"
        );
        let items = items(&v);
        // The task with a future threshold is hidden.
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Call the bank about the card",
                "Draft the quarterly plan",
                "Pick up the dry cleaning",
                "Buy coffee beans",
            ]
        );
        assert_eq!(items[0].tags, ["A", "+money", "@phone"]);
        assert_eq!(items[0].subtitle.as_deref(), Some("overdue 1d"));
    }

    #[test]
    fn done_tasks_and_limit() {
        let tasks = parse_todotxt(SAMPLE_TXT);
        let v = view(&tasks, sample_today(), "todo.txt", true, 30);
        let last = items(&v).last().unwrap();
        assert_eq!(last.title, "Water the plants");
        assert_eq!(last.status, Some(Status::Ok));
        assert!(last.tags.contains(&"done".to_string()));
        let v = view(&tasks, sample_today(), "todo.txt", true, 2);
        assert_eq!(items(&v).len(), 2);
        let v = view(&[], sample_today(), "todo.txt", false, 30);
        assert_eq!(v.summary, "todo.txt · 0 open");
    }

    #[test]
    fn due_texts() {
        let today = sample_today();
        assert_eq!(
            due_text(jiff::civil::date(2026, 10, 28), today),
            "due tomorrow"
        );
        assert_eq!(due_text(jiff::civil::date(2026, 12, 1), today), "due Dec 1");
    }

    #[tokio::test]
    async fn reads_the_file() {
        let dir = std::env::temp_dir().join(format!("dashboard-todo-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("list.md");
        std::fs::write(&path, "- [ ] one\n- [x] two\n").unwrap();
        let mut m = Todo::default();
        m.init(serde_json::json!({ "path": path })).await.unwrap();
        assert_eq!(
            m.render().await.unwrap().summary,
            "list.md · 1 open · 1 done"
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(m.render().await.is_err());
    }

    #[tokio::test]
    async fn config_errors() {
        for config in [
            serde_json::json!({}),
            serde_json::json!({ "path": "x", "limit": 0 }),
            serde_json::json!({ "path": "x", "format": "org" }),
        ] {
            assert!(Todo::default().init(config).await.is_err());
        }
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
