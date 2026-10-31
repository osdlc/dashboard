//! The dev bundle: GitHub pull requests and issues (through the `gh` CLI),
//! issue trackers (Jira, Linear, Forgejo) and local git repositories. Each
//! module sits behind a cargo feature named after its id.
//!
//! The trackers share one list per query, the same item layout, tokens from
//! the environment or a command, and relative ages. Tokens never go in the
//! config file. Each module names an environment variable (`token_env`) or a
//! command that prints the token (`token_command`, e.g. a password manager CLI).

use dashboard_contract::{
    Action, CONTRACT_VERSION, LayoutHints, ListItem, Manifest, Refresh, Size, Status, View, Widget,
};
use dashboard_modules_common::Builtin;
#[cfg(feature = "http")]
pub use dashboard_modules_common::http::{check, client};
pub use dashboard_modules_common::secret::Secret;
pub use dashboard_modules_common::time::{ago, demo_time, parse_time};
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "forgejo")]
pub mod forgejo;
#[cfg(feature = "git")]
pub mod git;
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
    #[cfg(feature = "git")]
    Builtin {
        id: "git",
        core: false,
        new: || Box::new(git::Git::default()),
        catalog: git::catalog,
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
        tone: None,
        title: issue.title,
        subtitle: Some(sub.join(" · ")),
        status: issue.status,
        tags: issue.tags,
        action: Some(Action::OpenUrl { url: issue.url }),
    }
}
