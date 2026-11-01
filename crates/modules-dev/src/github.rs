//! GitHub pull requests and issues, read with the `gh` CLI (uses its login).

use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CONTRACT_VERSION, CatalogEntry, LayoutHints, ListItem, Manifest, Refresh, Requirements,
    Size, Status, View, Widget,
};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::process::Command;

use crate::{ago, demo_time};

/// `gh search` uses the search API (30 requests a minute), so don't poll fast.
const INTERVAL_MS: u64 = 60_000;
const MIN_INTERVAL_MS: u64 = 15_000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Items per section.
    limit: u32,
    /// Extra `gh search` qualifiers for every section, e.g. `["--owner", "my-org"]`.
    filter: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            limit: 10,
            filter: vec![],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    number: u64,
    title: String,
    url: String,
    repository: Repo,
    updated_at: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repo {
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct Label {
    name: String,
}

#[derive(Clone, Copy)]
enum Kind {
    Prs,
    Issues,
}

/// One `gh search` query, shown as one list.
struct Section {
    label: &'static str,
    args: &'static [&'static str],
}

const PR_SECTIONS: &[Section] = &[
    Section {
        label: "Review requested",
        args: &["--review-requested=@me"],
    },
    Section {
        label: "Mine",
        args: &["--author=@me"],
    },
];
const ISSUE_SECTIONS: &[Section] = &[
    Section {
        label: "Assigned",
        args: &["--assignee=@me"],
    },
    Section {
        label: "Opened by me",
        args: &["--author=@me"],
    },
];

pub struct GitHub {
    kind: Kind,
    config: Config,
}

impl GitHub {
    pub fn prs() -> Self {
        Self {
            kind: Kind::Prs,
            config: Config::default(),
        }
    }

    pub fn issues() -> Self {
        Self {
            kind: Kind::Issues,
            config: Config::default(),
        }
    }

    async fn search(&self, section: &Section) -> Result<Vec<Item>> {
        let (sub, fields) = match self.kind {
            Kind::Prs => ("prs", "number,title,url,repository,updatedAt,isDraft"),
            Kind::Issues => ("issues", "number,title,url,repository,updatedAt,labels"),
        };
        let out = Command::new("gh")
            .args([
                "search",
                sub,
                "--state=open",
                "--sort=updated",
                "--json",
                fields,
            ])
            .arg(format!("--limit={}", self.config.limit))
            .args(section.args)
            .args(&self.config.filter)
            .stdin(Stdio::null())
            .output()
            .await
            .context("running gh (is the GitHub CLI installed?)")?;
        if !out.status.success() {
            bail!(
                "gh search {sub}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        serde_json::from_slice(&out.stdout).context("parsing gh output")
    }
}

#[async_trait]
impl Module for GitHub {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let (id, name, priority) = match self.kind {
            Kind::Prs => ("github-prs", "Pull requests", 50),
            Kind::Issues => ("github-issues", "Issues", 40),
        };
        Ok(Manifest {
            id: id.into(),
            name: name.into(),
            contract_version: CONTRACT_VERSION.into(),
            refresh: Refresh::Poll {
                interval_ms: INTERVAL_MS,
                min_interval_ms: Some(MIN_INTERVAL_MS),
            },
            layout: LayoutHints {
                size: Size::Medium,
                priority,
                min_size: Some(Size::Small),
            },
            commands: vec![],
            config_schema: None,
        })
    }

    async fn render(&mut self) -> Result<View> {
        let sections = match self.kind {
            Kind::Prs => PR_SECTIONS,
            Kind::Issues => ISSUE_SECTIONS,
        };
        let mut found = vec![];
        for section in sections {
            found.push((section.label, self.search(section).await?));
        }
        Ok(view(found))
    }
}

fn view(sections: Vec<(&str, Vec<Item>)>) -> View {
    let mut children = vec![];
    let mut counts = vec![];
    for (label, items) in sections {
        counts.push(format!("{} {}", items.len(), label.to_lowercase()));
        children.push(Widget::Text {
            text: format!("**{label}**"),
            markdown: true,
        });
        children.push(Widget::List {
            items: items.into_iter().map(list_item).collect(),
            empty_text: Some("none".into()),
        });
    }
    View {
        status: Status::Ok,
        summary: counts.join(" · "),
        root: Widget::Column { children },
    }
}

fn list_item(item: Item) -> ListItem {
    let mut tags: Vec<String> = item.labels.into_iter().map(|l| l.name).collect();
    if item.is_draft {
        tags.insert(0, "draft".into());
    }
    ListItem {
        tone: None,
        title: item.title,
        subtitle: Some(format!(
            "{}#{} · updated {}",
            item.repository.name_with_owner,
            item.number,
            ago(&item.updated_at)
        )),
        status: None,
        tags,
        action: Some(Action::OpenUrl { url: item.url }),
    }
}

// ---- catalog ----

/// A made-up item, shaped like `gh search` output.
fn demo_item(
    repo: &str,
    number: u64,
    title: &str,
    mins_ago: i64,
    draft: bool,
    labels: &[&str],
) -> Item {
    Item {
        number,
        title: title.into(),
        url: format!("https://github.com/{repo}/pull/{number}"),
        repository: Repo {
            name_with_owner: repo.into(),
        },
        updated_at: demo_time(mins_ago * 60),
        is_draft: draft,
        labels: labels
            .iter()
            .map(|l| Label {
                name: l.to_string(),
            })
            .collect(),
    }
}

fn github_entry(id: &str, name: &str, tag: &str, description: &str, demo: View) -> CatalogEntry {
    CatalogEntry {
        description: description.into(),
        tags: crate::strings(&["github", "dev", tag]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: crate::strings(&["gh"]),
            env: vec![],
            hosts: crate::strings(&["api.github.com"]),
        },
        ..crate::entry(id, name, Size::Medium, demo)
    }
}

pub fn catalog_prs() -> CatalogEntry {
    let demo = view(vec![
        (
            "Review requested",
            vec![
                demo_item(
                    "acme/api",
                    482,
                    "Add rate limiting to the search endpoint",
                    25,
                    false,
                    &[],
                ),
                demo_item(
                    "acme/web",
                    1310,
                    "Move settings page to the new form components",
                    190,
                    false,
                    &[],
                ),
            ],
        ),
        (
            "Mine",
            vec![
                demo_item(
                    "acme/api",
                    479,
                    "Retry webhook deliveries with backoff",
                    75,
                    false,
                    &[],
                ),
                demo_item("acme/infra", 88, "Upgrade Postgres to 17", 2_900, true, &[]),
            ],
        ),
    ]);
    github_entry(
        "github-prs",
        "Pull requests",
        "pull-requests",
        "Open pull requests waiting for your review, and your own. Reads them with the gh CLI and its login.",
        demo,
    )
}

pub fn catalog_issues() -> CatalogEntry {
    let mut assigned = vec![
        demo_item(
            "acme/web",
            1298,
            "Dark mode loses contrast on charts",
            140,
            false,
            &["bug", "ui"],
        ),
        demo_item(
            "acme/api",
            455,
            "Document the export API",
            4_400,
            false,
            &["docs"],
        ),
    ];
    let mut opened = vec![demo_item(
        "acme/infra",
        91,
        "Nightly backup job is slow",
        600,
        false,
        &["ops"],
    )];
    for item in assigned.iter_mut().chain(opened.iter_mut()) {
        item.url = item.url.replace("/pull/", "/issues/");
    }
    let demo = view(vec![("Assigned", assigned), ("Opened by me", opened)]);
    github_entry(
        "github-issues",
        "Issues",
        "issues",
        "Open issues assigned to you, and the ones you opened. Reads them with the gh CLI and its login.",
        demo,
    )
}
