//! Local git repositories: branch, commits ahead of and behind the upstream,
//! and uncommitted changes. Runs `git status --porcelain=v2 --branch` once per
//! repository:
//!
//! ```yaml
//! modules:
//!   git:
//!     config:
//!       repos: [~/dev/api, ~/dev/web]
//! ```
//!
//! The same widget is also written as a TypeScript plugin
//! (`plugins/ext/git-ts`) and a Python plugin (`plugins/ext/git-py`) for the
//! native-vs-plugin benchmark. All three show the same view for the same
//! repositories, so keep them in step.

use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    CatalogEntry, Cell, Column, Manifest, Requirements, Row, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::{expand_home, manifest};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Repositories to show, by path. `~` is your home folder.
    repos: Vec<String>,
}

/// One repository, as `git status --porcelain=v2 --branch` describes it.
#[derive(Debug, Default, PartialEq)]
struct Repo {
    /// "detached" when HEAD is not on a branch.
    branch: String,
    upstream: Upstream,
    /// Changed, staged, conflicted and untracked files.
    changed: usize,
}

#[derive(Debug, Default, PartialEq)]
enum Upstream {
    #[default]
    None,
    /// The upstream is set but its branch no longer exists.
    Gone,
    Tracking {
        ahead: u64,
        behind: u64,
    },
}

/// A repository's row: what git said, or why it couldn't be read.
struct Found {
    name: String,
    repo: Result<Repo, String>,
}

#[derive(Default)]
pub struct Git {
    config: Config,
}

#[async_trait]
impl Module for Git {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        if self.config.repos.is_empty() {
            bail!("the git module needs `config: {{ repos: [path, ...] }}`");
        }
        Ok(manifest("git", "Git", 30_000, 5_000, Size::Medium, 20))
    }

    async fn render(&mut self) -> Result<View> {
        let mut found = vec![];
        for path in &self.config.repos {
            let path = expand_home(path);
            found.push(Found {
                name: repo_name(&path),
                repo: status(&path).await?,
            });
        }
        Ok(view(found))
    }
}

/// The folder's name, after following links.
fn repo_name(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.to_string_lossy().into_owned(),
    }
}

/// Runs git in one repository. The outer error means git itself could not
/// run; the inner one is this repository's problem, shown in its row.
async fn status(path: &Path) -> Result<Result<Repo, String>> {
    if !path.is_dir() {
        return Ok(Err("not a folder".into()));
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain=v2", "--branch"])
        // Don't take the index lock: the user's own git commands come first.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        // Stop git if the host gives up on this render.
        .kill_on_drop(true)
        .output()
        .await
        .context("running git (is it installed?)")?;
    if !out.status.success() {
        return Ok(Err(git_error(&String::from_utf8_lossy(&out.stderr))));
    }
    Ok(Ok(parse(&String::from_utf8_lossy(&out.stdout))))
}

/// The first line of git's complaint, without "fatal: ".
fn git_error(stderr: &str) -> String {
    let line = stderr.lines().next().unwrap_or("").trim();
    let line = line.strip_prefix("fatal: ").unwrap_or(line);
    if line.is_empty() {
        "git status failed".into()
    } else {
        line.into()
    }
}

/// Reads `git status --porcelain=v2 --branch` output.
fn parse(out: &str) -> Repo {
    let mut repo = Repo::default();
    let mut has_upstream = false;
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            repo.branch = if head == "(detached)" {
                "detached".into()
            } else {
                head.into()
            };
        } else if line.starts_with("# branch.upstream ") {
            has_upstream = true;
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut counts = ab
                .split_whitespace()
                .map(|n| n.trim_start_matches(['+', '-']).parse().unwrap_or(0));
            repo.upstream = Upstream::Tracking {
                ahead: counts.next().unwrap_or(0),
                behind: counts.next().unwrap_or(0),
            };
        } else if !line.starts_with('#') && !line.starts_with('!') && !line.is_empty() {
            repo.changed += 1;
        }
    }
    // An upstream with no ahead/behind line is one whose branch is gone.
    if has_upstream && repo.upstream == Upstream::None {
        repo.upstream = Upstream::Gone;
    }
    repo
}

fn view(found: Vec<Found>) -> View {
    let (mut dirty, mut unpushed, mut behind) = (0, 0, 0);
    let mut worst = Status::Ok;
    let mut rows = vec![];
    for f in &found {
        let (branch, changes, sync, status) = match &f.repo {
            Ok(r) => {
                let (ahead, back) = match r.upstream {
                    Upstream::Tracking { ahead, behind } => (ahead, behind),
                    _ => (0, 0),
                };
                dirty += usize::from(r.changed > 0);
                unpushed += usize::from(ahead > 0);
                behind += usize::from(back > 0);
                let status = if r.changed > 0 || back > 0 {
                    Status::Warn
                } else {
                    Status::Ok
                };
                (
                    r.branch.clone(),
                    changes(r.changed),
                    sync(&r.upstream),
                    status,
                )
            }
            Err(e) => ("-".into(), e.clone(), "-".into(), Status::Error),
        };
        worst = worse(worst, status);
        let cells = [
            ("repo", f.name.clone()),
            ("branch", branch),
            ("changes", changes),
            ("sync", sync),
        ];
        rows.push(Row {
            cells: cells
                .into_iter()
                .map(|(k, v)| (k.to_string(), Cell::Text(v)))
                .collect(),
            status: Some(status),
            action: None,
        });
    }
    let columns = [
        ("repo", "Repository"),
        ("branch", "Branch"),
        ("changes", "Changes"),
        ("sync", "Upstream"),
    ];
    View {
        status: worst,
        summary: format!(
            "{} {} · {dirty} dirty · {unpushed} unpushed · {behind} behind",
            found.len(),
            if found.len() == 1 { "repo" } else { "repos" }
        ),
        root: Widget::Table {
            columns: columns
                .into_iter()
                .map(|(key, label)| Column {
                    key: key.into(),
                    label: label.into(),
                })
                .collect(),
            rows,
        },
    }
}

fn changes(n: usize) -> String {
    if n == 0 {
        "clean".into()
    } else {
        format!("{n} changed")
    }
}

fn sync(upstream: &Upstream) -> String {
    match *upstream {
        Upstream::None => "no upstream".into(),
        Upstream::Gone => "upstream gone".into(),
        Upstream::Tracking {
            ahead: 0,
            behind: 0,
        } => "in sync".into(),
        Upstream::Tracking { ahead, behind } => {
            let mut parts = vec![];
            if ahead > 0 {
                parts.push(format!("↑{ahead}"));
            }
            if behind > 0 {
                parts.push(format!("↓{behind}"));
            }
            parts.join(" ")
        }
    }
}

fn worse(a: Status, b: Status) -> Status {
    let rank = |s: Status| match s {
        Status::Error => 3,
        Status::Warn => 2,
        Status::Ok => 1,
        _ => 0,
    };
    if rank(b) > rank(a) { b } else { a }
}

// ---- catalog ----

fn demo() -> View {
    let repo = |branch: &str, upstream, changed| {
        Ok(Repo {
            branch: branch.into(),
            upstream,
            changed,
        })
    };
    view(vec![
        Found {
            name: "api".into(),
            repo: repo(
                "rate-limits",
                Upstream::Tracking {
                    ahead: 2,
                    behind: 0,
                },
                3,
            ),
        },
        Found {
            name: "web".into(),
            repo: repo(
                "main",
                Upstream::Tracking {
                    ahead: 0,
                    behind: 4,
                },
                0,
            ),
        },
        Found {
            name: "infra".into(),
            repo: repo(
                "main",
                Upstream::Tracking {
                    ahead: 0,
                    behind: 0,
                },
                0,
            ),
        },
        Found {
            name: "notes".into(),
            repo: repo("drafts", Upstream::None, 1),
        },
    ])
}

pub fn catalog() -> CatalogEntry {
    CatalogEntry {
        description: "Local git repositories: branch, commits ahead of and behind the upstream, and uncommitted changes. Runs git status in each one.".into(),
        tags: crate::strings(&["git", "dev", "local"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["git"]),
            env: vec![],
            hosts: vec![],
        },
        ..crate::entry("git", "Git", Size::Medium, demo())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-written `git status --porcelain=v2 --branch` output: two commits
    /// ahead, one behind, a modified file, a rename, a conflict, an untracked
    /// file and an ignored one (which doesn't count).
    const SAMPLE: &str = "\
# branch.oid 5b1d0e7c2f4a9e6d3c8b7a6f5e4d3c2b1a0f9e8d
# branch.head feature/login
# branch.upstream origin/feature/login
# branch.ab +2 -1
1 .M N... 100644 100644 100644 3f2a1b 3f2a1b src/main.rs
2 R. N... 100644 100644 100644 9c8d7e 9c8d7e R100 src/new.rs\tsrc/old.rs
u UU N... 100644 100644 100644 100644 1a2b3c 4d5e6f 7a8b9c Cargo.lock
? notes.txt
! target/
";

    #[test]
    fn parses_branch_counts_and_changes() {
        assert_eq!(
            parse(SAMPLE),
            Repo {
                branch: "feature/login".into(),
                upstream: Upstream::Tracking {
                    ahead: 2,
                    behind: 1
                },
                changed: 4,
            }
        );
    }

    #[test]
    fn parses_detached_no_upstream_and_gone() {
        let detached = parse("# branch.oid abc\n# branch.head (detached)\n");
        assert_eq!(detached.branch, "detached");
        assert_eq!(detached.upstream, Upstream::None);
        assert_eq!(detached.changed, 0);
        let gone = parse("# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n");
        assert_eq!(gone.upstream, Upstream::Gone);
    }

    #[test]
    fn git_errors_lose_the_fatal_prefix() {
        assert_eq!(
            git_error("fatal: not a git repository (or any of the parent directories): .git\n"),
            "not a git repository (or any of the parent directories): .git"
        );
        assert_eq!(git_error(""), "git status failed");
    }

    #[test]
    fn view_counts_and_statuses() {
        let view = view(vec![
            Found {
                name: "app".into(),
                repo: Ok(parse(SAMPLE)),
            },
            Found {
                name: "gone".into(),
                repo: Err("not a folder".into()),
            },
        ]);
        assert_eq!(view.status, Status::Error);
        assert_eq!(view.summary, "2 repos · 1 dirty · 1 unpushed · 1 behind");
        let Widget::Table { rows, .. } = &view.root else {
            panic!("table")
        };
        let text = |r: &Row, k: &str| match &r.cells[k] {
            Cell::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(text(&rows[0], "branch"), "feature/login");
        assert_eq!(text(&rows[0], "changes"), "4 changed");
        assert_eq!(text(&rows[0], "sync"), "↑2 ↓1");
        assert_eq!(rows[0].status, Some(Status::Warn));
        assert_eq!(text(&rows[1], "changes"), "not a folder");
        assert_eq!(rows[1].status, Some(Status::Error));
    }

    #[tokio::test]
    async fn needs_repos() {
        let err = Git::default().init(serde_json::json!({})).await;
        assert!(err.is_err());
    }
}
