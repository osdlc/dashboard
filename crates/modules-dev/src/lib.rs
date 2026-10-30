//! The dev bundle: GitHub pull requests and issues (through the `gh` CLI) and
//! issue trackers (Jira, Linear, Forgejo). Each module sits behind a cargo
//! feature named after its id.
//!
//! The trackers share one list per query, the same item layout, tokens from
//! the environment or a command, and relative ages. Tokens never go in the
//! config file. Each module names an environment variable (`token_env`) or a
//! command that prints the token (`token_command`, e.g. a password manager CLI).

use anyhow::{Context, Result, bail};
use dashboard_contract::{
    Action, CONTRACT_VERSION, LayoutHints, ListItem, Manifest, Refresh, Size, Status, View, Widget,
};
use dashboard_modules_common::Builtin;
#[cfg(feature = "http")]
pub use dashboard_modules_common::http::{check, client};
pub use dashboard_modules_common::time::{ago, demo_time, parse_time};
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};
use schemars::JsonSchema;
use serde::Deserialize;

#[cfg(feature = "forgejo")]
pub mod forgejo;
#[cfg(any(feature = "github-prs", feature = "github-issues"))]
pub mod github;
#[cfg(feature = "jira")]
pub mod jira;
#[cfg(feature = "linear")]
pub mod linear;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "github-prs")]
    Builtin {
        id: "github-prs",
        core: true,
        new: || Box::new(github::GitHub::prs()),
        catalog: github::catalog_prs,
    },
    #[cfg(feature = "github-issues")]
    Builtin {
        id: "github-issues",
        core: true,
        new: || Box::new(github::GitHub::issues()),
        catalog: github::catalog_issues,
    },
    #[cfg(feature = "jira")]
    Builtin {
        id: "jira",
        core: false,
        new: || Box::new(jira::Jira::default()),
        catalog: jira::catalog,
    },
    #[cfg(feature = "linear")]
    Builtin {
        id: "linear",
        core: false,
        new: || Box::new(linear::Linear::default()),
        catalog: linear::catalog,
    },
    #[cfg(feature = "forgejo")]
    Builtin {
        id: "forgejo",
        core: false,
        new: || Box::new(forgejo::Forgejo::default()),
        catalog: forgejo::catalog,
    },
];

/// Tracker APIs are rate-limited and issues change slowly.
const INTERVAL_MS: u64 = 120_000;
const MIN_INTERVAL_MS: u64 = 30_000;

/// One issue or pull request, as every tracker shows it.
#[derive(Debug)]
pub struct Issue {
    /// "ENG-12", "owner/repo#4".
    pub key: String,
    pub title: String,
    pub url: String,
    pub state: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    /// Colors the item's marker, e.g. from priority.
    pub status: Option<Status>,
}

/// Where a module gets its API token.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Secret {
    /// Environment variable holding the token.
    pub token_env: Option<String>,
    /// Command that prints the token, e.g. `["op", "read", "op://dev/linear/token"]`.
    pub token_command: Option<Vec<String>>,
}

impl Secret {
    /// Reads the token: the configured command, else the variable (or `default_env`).
    pub async fn token(&self, default_env: &str) -> Result<String> {
        if let Some(argv) = &self.token_command {
            let (program, args) = argv.split_first().context("token_command is empty")?;
            let out = tokio::process::Command::new(program)
                .args(args)
                .stdin(std::process::Stdio::null())
                .output()
                .await
                .with_context(|| format!("running token_command {program:?}"))?;
            if !out.status.success() {
                bail!(
                    "token_command {program:?} failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if token.is_empty() {
                bail!("token_command {program:?} printed nothing");
            }
            return Ok(token);
        }
        let var = self.token_env.as_deref().unwrap_or(default_env);
        match std::env::var(var) {
            Ok(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
            _ => bail!(
                "no API token: set ${var} before starting the dashboard, or set token_env / token_command in the config"
            ),
        }
    }
}

pub fn manifest(id: &str, name: &str) -> Manifest {
    Manifest {
        id: id.into(),
        name: name.into(),
        contract_version: CONTRACT_VERSION.into(),
        refresh: Refresh::Poll {
            interval_ms: INTERVAL_MS,
            min_interval_ms: Some(MIN_INTERVAL_MS),
        },
        layout: LayoutHints {
            size: Size::Medium,
            priority: 30,
            min_size: Some(Size::Small),
        },
        commands: vec![],
        config_schema: None,
    }
}

/// A heading and list per section, like the GitHub modules.
pub fn sections_view(sections: Vec<(String, Vec<Issue>)>) -> View {
    let mut children = vec![];
    let mut counts = vec![];
    for (label, issues) in sections {
        counts.push(format!("{} {}", issues.len(), label.to_lowercase()));
        children.push(Widget::Text {
            text: format!("**{label}**"),
            markdown: true,
        });
        children.push(Widget::List {
            items: issues.into_iter().map(list_item).collect(),
            empty_text: Some("none".into()),
        });
    }
    View {
        status: Status::Ok,
        summary: counts.join(" · "),
        root: Widget::Column { children },
    }
}

fn list_item(issue: Issue) -> ListItem {
    let mut sub = vec![issue.key];
    sub.extend(issue.state);
    if let Some(updated) = &issue.updated {
        sub.push(format!("updated {}", ago(updated)));
    }
    ListItem {
        title: issue.title,
        subtitle: Some(sub.join(" · ")),
        status: issue.status,
        tags: issue.tags,
        action: Some(Action::OpenUrl { url: issue.url }),
    }
}
