//! What the calendar and task modules share: dates, the task list every task
//! module shows (overdue first, then by date) and finding files.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use dashboard_contract::{Action, ListItem, Status, View, Widget};
use jiff::civil::Date;

use crate::expand_home;

/// Today in the system time zone.
pub(crate) fn today() -> Date {
    jiff::Zoned::now().date()
}

/// `YYYY-MM-DD` at the start of `s`.
pub(crate) fn parse_date(s: &str) -> Option<Date> {
    s.get(..10)?.parse().ok()
}

/// "today", "tomorrow", "2d ago", "Fri", "30 Oct", "30 Oct 2027".
pub(crate) fn day_text(date: Date, today: Date) -> String {
    let days = (date - today).get_days();
    match days {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        n if n < 0 => format!("{}d ago", -n),
        2..=6 => date.strftime("%a").to_string(),
        _ if date.year() == today.year() => date.strftime("%-d %b").to_string(),
        _ => date.strftime("%-d %b %Y").to_string(),
    }
}

/// One task, as every task module shows it.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Task {
    pub title: String,
    /// The deadline. Past it, the task shows red.
    pub due: Option<Date>,
    /// When to start (org SCHEDULED, Obsidian ⏳).
    pub scheduled: Option<Date>,
    /// Sorts tasks on the same day: lower first (priority A, ⏫, H...).
    pub rank: i32,
    pub tags: Vec<String>,
    /// Where it came from, e.g. the file name.
    pub source: Option<String>,
    pub action: Option<Action>,
}

impl Task {
    /// The date the task sorts by: its deadline or start, whichever is first.
    pub(crate) fn when(&self) -> Option<Date> {
        match (self.due, self.scheduled) {
            (Some(d), Some(s)) => Some(d.min(s)),
            (d, s) => d.or(s),
        }
    }

    fn overdue(&self, today: Date) -> bool {
        self.due.is_some_and(|d| d < today)
    }

    fn due_today(&self, today: Date) -> bool {
        self.due == Some(today)
    }
}

/// Dated tasks first, earliest first; then by rank and title.
pub(crate) fn sort_tasks(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        (a.when().is_none(), a.when(), a.rank, &a.title).cmp(&(
            b.when().is_none(),
            b.when(),
            b.rank,
            &b.title,
        ))
    });
}

/// A list of tasks, in the order given, showing at most `limit`. Overdue
/// tasks are red, tasks due today yellow.
pub(crate) fn tasks_view(tasks: &[Task], today: Date, limit: usize) -> View {
    let overdue = tasks.iter().filter(|t| t.overdue(today)).count();
    let due_today = tasks.iter().filter(|t| t.due_today(today)).count();
    let mut summary = vec![];
    if overdue > 0 {
        summary.push(format!("{overdue} overdue"));
    }
    if due_today > 0 {
        summary.push(format!("{due_today} due today"));
    }
    summary.push(format!("{} open", tasks.len()));
    let items = tasks
        .iter()
        .take(limit)
        .map(|t| {
            let mut sub = vec![];
            if let Some(d) = t.due {
                sub.push(match (d - today).get_days() {
                    n if n < 0 => format!("overdue {}d", -n),
                    _ => format!("due {}", day_text(d, today)),
                });
            }
            if let Some(s) = t.scheduled.filter(|s| t.due.is_none_or(|d| *s < d)) {
                sub.push(format!("scheduled {}", day_text(s, today)));
            }
            sub.extend(t.source.clone());
            let status = if t.overdue(today) {
                Some(Status::Error)
            } else if t.due_today(today) {
                Some(Status::Warn)
            } else {
                None
            };
            ListItem {
                title: t.title.clone(),
                subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                status,
                tags: t.tags.clone(),
                action: t.action.clone(),
            }
        })
        .collect();
    View {
        status: if overdue > 0 {
            Status::Warn
        } else {
            Status::Ok
        },
        summary: summary.join(" · "),
        root: Widget::List {
            items,
            empty_text: Some("nothing to do".into()),
        },
    }
}

/// Files under `roots` (files or folders, searched recursively) with one of
/// `exts`, each with the root it was found under. Skips hidden folders and
/// `node_modules`.
pub(crate) fn find_files(roots: &[String], exts: &[&str]) -> Result<Vec<(PathBuf, PathBuf)>> {
    if roots.is_empty() {
        bail!("no `paths` set");
    }
    let mut out = vec![];
    for root in roots {
        let root = expand_home(root);
        if root.is_file() {
            let dir = root.parent().unwrap_or(Path::new("")).to_path_buf();
            out.push((root, dir));
        } else if root.is_dir() {
            walk(&root, &root, exts, 0, &mut out);
        } else {
            bail!("{} does not exist", root.display());
        }
    }
    Ok(out)
}

/// How deep `find_files` goes, so a mistaken `paths: [~]` stays quick.
const MAX_DEPTH: usize = 8;

fn walk(root: &Path, dir: &Path, exts: &[&str], depth: usize, out: &mut Vec<(PathBuf, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &path, exts, depth + 1, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
        {
            out.push((path, root.to_path_buf()));
        }
    }
}

/// An `open_url` action from a `link` template such as
/// `https://github.com/me/notes/blob/main/{path}#L{line}`, where `{path}` is
/// the file's path under `root`. Local files can't be opened from a tile, so
/// with no template there is no action.
pub(crate) fn link(
    template: Option<&str>,
    root: &Path,
    file: &Path,
    line: usize,
) -> Option<Action> {
    let template = template?;
    let rel = file.strip_prefix(root).unwrap_or(file);
    let rel = rel
        .components()
        .map(|c| encode(&c.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join("/");
    Some(Action::OpenUrl {
        url: template
            .replace("{path}", &rel)
            .replace("{line}", &line.to_string()),
    })
}

/// A path segment for a URL: everything but letters, digits and `-._~`
/// percent-encoded as UTF-8 (`My notes & ideas` -> `My%20notes%20%26%20ideas`).
fn encode(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The file name, for a task's subtitle.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn day_texts() {
        let today = date(2026, 10, 27);
        assert_eq!(day_text(today, today), "today");
        assert_eq!(day_text(date(2026, 10, 28), today), "tomorrow");
        assert_eq!(day_text(date(2026, 10, 24), today), "3d ago");
        assert_eq!(day_text(date(2026, 10, 30), today), "Fri");
        assert_eq!(day_text(date(2026, 11, 30), today), "30 Nov");
        assert_eq!(day_text(date(2027, 1, 2), today), "2 Jan 2027");
    }

    #[test]
    fn overdue_first_then_by_date_then_undated() {
        let today = date(2026, 10, 27);
        let t = |title: &str, due: Option<Date>, rank| Task {
            title: title.into(),
            due,
            rank,
            ..Task::default()
        };
        let mut tasks = vec![
            t("later", Some(date(2026, 11, 2)), 0),
            t("undated", None, 0),
            t("late", Some(date(2026, 10, 20)), 0),
            t("today b", Some(today), 2),
            t("today a", Some(today), 1),
        ];
        sort_tasks(&mut tasks);
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["late", "today a", "today b", "later", "undated"]);
        let view = tasks_view(&tasks, today, 10);
        assert_eq!(view.summary, "1 overdue · 2 due today · 5 open");
        assert_eq!(view.status, Status::Warn);
        let Widget::List { items, .. } = &view.root else {
            panic!()
        };
        assert_eq!(items[0].status, Some(Status::Error));
        assert_eq!(items[0].subtitle.as_deref(), Some("overdue 7d"));
        assert_eq!(items[1].status, Some(Status::Warn));
    }

    #[test]
    fn links_from_a_template() {
        let a = link(
            Some("https://example.com/notes/{path}#L{line}"),
            Path::new("/n"),
            Path::new("/n/work/My tasks.md"),
            4,
        );
        assert_eq!(
            a,
            Some(Action::OpenUrl {
                url: "https://example.com/notes/work/My%20tasks.md#L4".into()
            })
        );
        assert_eq!(link(None, Path::new("/n"), Path::new("/n/a.md"), 1), None);
        assert_eq!(
            link(
                Some("obsidian://open?vault=Notes&file={path}"),
                Path::new("/n"),
                Path::new("/n/Q&A #2/Café?.md"),
                1
            ),
            Some(Action::OpenUrl {
                url: "obsidian://open?vault=Notes&file=Q%26A%20%232/Caf%C3%A9%3F.md".into()
            })
        );
    }
}
