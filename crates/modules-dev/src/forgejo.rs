//! Forgejo (and Gitea) issues and pull requests across every repository you
//! can see, e.g. on Codeberg.
//!
//! ```yaml
//! modules:
//!   forgejo:
//!     config: { url: https://codeberg.org, token_env: FORGEJO_TOKEN }
//! ```

use anyhow::{Context, Result};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, View};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{Issue, Secret, check, client, demo_time, manifest, sections_view};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Section {
    /// Issues assigned to you.
    Assigned,
    /// Issues you opened.
    Created,
    /// Pull requests you opened.
    MyPulls,
    /// Pull requests waiting for your review.
    ReviewRequested,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Section::Assigned => "Assigned",
            Section::Created => "Opened by me",
            Section::MyPulls => "My pull requests",
            Section::ReviewRequested => "Review requested",
        }
    }

    /// Query parameters for `/repos/issues/search`.
    fn params(self) -> [(&'static str, &'static str); 2] {
        match self {
            Section::Assigned => [("type", "issues"), ("assigned", "true")],
            Section::Created => [("type", "issues"), ("created", "true")],
            Section::MyPulls => [("type", "pulls"), ("created", "true")],
            Section::ReviewRequested => [("type", "pulls"), ("review_requested", "true")],
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Instance URL, e.g. `https://codeberg.org`.
    url: Option<String>,
    #[serde(flatten)]
    secret: Secret,
    limit: u32,
    sections: Vec<Section>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: None,
            secret: Secret::default(),
            limit: 10,
            sections: vec![
                Section::ReviewRequested,
                Section::MyPulls,
                Section::Assigned,
            ],
        }
    }
}

#[derive(Default)]
pub struct Forgejo {
    config: Config,
    base: String,
}

#[async_trait]
impl Module for Forgejo {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let url = self
            .config
            .url
            .as_deref()
            .context("the forgejo module needs `config: { url: https://codeberg.org }`")?;
        self.base = url.trim_end_matches('/').to_string();
        Ok(manifest("forgejo", "Forgejo"))
    }

    async fn render(&mut self) -> Result<View> {
        let token = self.config.secret.token("FORGEJO_TOKEN").await?;
        let http = client()?;
        let mut sections = vec![];
        for section in &self.config.sections {
            let limit = self.config.limit.to_string();
            let resp = http
                .get(format!("{}/api/v1/repos/issues/search", self.base))
                .header("Authorization", format!("token {token}"))
                .query(&[("state", "open"), ("limit", limit.as_str())])
                .query(&section.params())
                .send()
                .await?;
            let json: serde_json::Value = check(resp).await?.json().await?;
            sections.push((section.label().to_string(), parse(&json)?));
        }
        Ok(sections_view(sections))
    }
}

#[derive(Deserialize)]
struct Item {
    number: u64,
    title: String,
    html_url: String,
    updated_at: Option<String>,
    repository: Option<Repo>,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Repo {
    full_name: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

fn parse(json: &serde_json::Value) -> Result<Vec<Issue>> {
    let items: Vec<Item> =
        serde_json::from_value(json.clone()).context("unexpected Forgejo response")?;
    Ok(items
        .into_iter()
        .map(|i| Issue {
            key: match i.repository {
                Some(r) => format!("{}#{}", r.full_name, i.number),
                None => format!("#{}", i.number),
            },
            title: i.title,
            url: i.html_url,
            state: None,
            updated: i.updated_at,
            tags: i.labels.into_iter().map(|l| l.name).collect(),
            status: None,
        })
        .collect())
}

pub fn catalog() -> CatalogEntry {
    // A hand-written `/repos/issues/search` reply, as the Forgejo API documents it.
    let item =
        |repo: &str, kind: &str, number: u64, title: &str, hours_ago: i64, labels: &[&str]| {
            let labels: Vec<_> = labels
                .iter()
                .map(|l| serde_json::json!({ "name": l }))
                .collect();
            serde_json::json!({
                "number": number, "title": title,
                "html_url": format!("https://codeberg.org/{repo}/{kind}/{number}"),
                "updated_at": demo_time(hours_ago * 3600), "repository": { "full_name": repo },
                "labels": labels
            })
        };
    let review = serde_json::json!([item(
        "acme/notes",
        "pulls",
        57,
        "Sync folders in the background",
        3,
        &[]
    ),]);
    let mine = serde_json::json!([
        item(
            "acme/notes",
            "pulls",
            55,
            "Export notes as Markdown",
            30,
            &["feature"]
        ),
        item(
            "me/dotfiles",
            "pulls",
            12,
            "Switch the shell prompt to starship",
            98,
            &[]
        ),
    ]);
    let assigned = serde_json::json!([item(
        "acme/notes",
        "issues",
        49,
        "Search ignores accented letters",
        8,
        &["bug"]
    ),]);
    let sections = [
        (Section::ReviewRequested, review),
        (Section::MyPulls, mine),
        (Section::Assigned, assigned),
    ]
    .into_iter()
    .map(|(s, json)| {
        (
            s.label().to_string(),
            parse(&json).expect("demo data parses"),
        )
    })
    .collect();
    CatalogEntry {
        description: "Forgejo and Gitea issues and pull requests across your repositories, e.g. \
                      on Codeberg. Untested live: built from the API docs with sample data."
            .into(),
        tags: crate::strings(&["issues", "pull-requests", "dev", "untested-live"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: crate::strings(&["FORGEJO_TOKEN"]),
            hosts: crate::strings(&["codeberg.org (or your instance)"]),
        },
        ..crate::entry("forgejo", "Forgejo", Size::Medium, sections_view(sections))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results() {
        let json = serde_json::json!([{
            "number": 4, "title": "Add dark mode", "html_url": "https://codeberg.org/me/app/issues/4",
            "updated_at": "2026-10-26T21:09:57-05:00", "repository": { "full_name": "me/app" },
            "labels": [{ "name": "ui" }]
        }]);
        let issues = parse(&json).unwrap();
        assert_eq!(issues[0].key, "me/app#4");
        assert_eq!(issues[0].tags, ["ui"]);
    }
}
