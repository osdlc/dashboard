//! Jira issues from JQL searches. Jira Cloud uses your email and an API token
//! (basic auth); Jira Server / Data Center uses a personal access token (`auth: bearer`).
//!
//! ```yaml
//! modules:
//!   jira:
//!     config:
//!       url: https://acme.atlassian.net
//!       email: me@acme.com
//!       token_env: JIRA_API_TOKEN
//!       sections:
//!         - { label: Mine, jql: "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC" }
//! ```

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, Status, View};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{Issue, Secret, check, client, demo_time, manifest, sections_view};

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Auth {
    /// Jira Cloud: email + API token.
    #[default]
    Basic,
    /// Server / Data Center: personal access token.
    Bearer,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Section {
    label: String,
    jql: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Site URL, e.g. `https://acme.atlassian.net`.
    url: Option<String>,
    /// Account email, for `auth: basic`.
    email: Option<String>,
    auth: Auth,
    #[serde(flatten)]
    secret: Secret,
    limit: u32,
    sections: Vec<Section>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: None,
            email: None,
            auth: Auth::Basic,
            secret: Secret::default(),
            limit: 10,
            sections: vec![Section {
                label: "Assigned".into(),
                jql: "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC"
                    .into(),
            }],
        }
    }
}

#[derive(Default)]
pub struct Jira {
    config: Config,
    base: String,
}

#[async_trait]
impl Module for Jira {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let url = self
            .config
            .url
            .as_deref()
            .context("the jira module needs `config: { url: https://<site>.atlassian.net }`")?;
        if self.config.auth == Auth::Basic && self.config.email.is_none() {
            bail!(
                "the jira module needs `email` for Jira Cloud (or `auth: bearer` for Server / Data Center)"
            );
        }
        self.base = url.trim_end_matches('/').to_string();
        Ok(manifest("jira", "Jira"))
    }

    async fn render(&mut self) -> Result<View> {
        let token = self.config.secret.token("JIRA_API_TOKEN").await?;
        let http = client()?;
        // Cloud's current search endpoint; Server / DC only has the v2 one.
        let endpoint = match self.config.auth {
            Auth::Basic => "/rest/api/3/search/jql",
            Auth::Bearer => "/rest/api/2/search",
        };
        let mut sections = vec![];
        for section in &self.config.sections {
            let req = http.get(format!("{}{endpoint}", self.base)).query(&[
                ("jql", section.jql.as_str()),
                ("fields", "summary,status,updated,priority,labels"),
                ("maxResults", &self.config.limit.to_string()),
            ]);
            let req = match self.config.auth {
                Auth::Basic => req.basic_auth(
                    self.config.email.as_deref().unwrap_or_default(),
                    Some(&token),
                ),
                Auth::Bearer => req.bearer_auth(&token),
            };
            let json: serde_json::Value = check(req.send().await?).await?.json().await?;
            sections.push((section.label.clone(), parse(&json, &self.base)?));
        }
        Ok(sections_view(sections))
    }
}

#[derive(Deserialize)]
struct Found {
    issues: Vec<JiraIssue>,
}

#[derive(Deserialize)]
struct JiraIssue {
    key: String,
    fields: Fields,
}

#[derive(Deserialize)]
struct Fields {
    summary: String,
    status: Option<Named>,
    updated: Option<String>,
    priority: Option<Named>,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

fn parse(json: &serde_json::Value, base: &str) -> Result<Vec<Issue>> {
    let found: Found = serde_json::from_value(json.clone()).context("unexpected Jira response")?;
    Ok(found
        .issues
        .into_iter()
        .map(|i| {
            let priority = i.fields.priority.map(|p| p.name);
            let status = match priority.as_deref() {
                Some("Highest" | "Blocker" | "Critical") => Some(Status::Error),
                Some("High" | "Major") => Some(Status::Warn),
                _ => None,
            };
            let mut tags = i.fields.labels;
            if let Some(p) = priority.filter(|p| p != "Medium") {
                tags.insert(0, p);
            }
            Issue {
                url: format!("{base}/browse/{}", i.key),
                key: i.key,
                title: i.fields.summary,
                state: i.fields.status.map(|s| s.name),
                updated: i.fields.updated,
                tags,
                status,
            }
        })
        .collect())
}

pub fn catalog() -> CatalogEntry {
    // A hand-written search response in the shape the Jira Cloud API documents.
    let issue = |key: &str,
                 summary: &str,
                 status: &str,
                 hours_ago: i64,
                 priority: &str,
                 labels: &[&str]| {
        serde_json::json!({ "key": key, "fields": {
            "summary": summary, "status": { "name": status },
            "updated": demo_time(hours_ago * 3600), "priority": { "name": priority }, "labels": labels
        }})
    };
    let base = "https://acme.atlassian.net";
    let mine = serde_json::json!({ "issues": [
        issue("OPS-212", "Rotate the staging TLS certificates", "In Progress", 2, "High", &["infra"]),
        issue("OPS-198", "Alert on failed nightly backups", "To Do", 26, "Medium", &["monitoring"]),
        issue("WEB-731", "Checkout button overlaps the footer on small screens", "In Review", 51, "Highest", &["bug", "mobile"]),
    ]});
    let issues = parse(&mine, base).expect("demo data parses");
    CatalogEntry {
        description:
            "Jira issues from your own JQL searches, in Jira Cloud or Server. Untested live: \
                      built from the API docs with sample data."
                .into(),
        tags: crate::strings(&["issues", "dev", "atlassian", "untested-live"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: crate::strings(&["JIRA_API_TOKEN"]),
            hosts: crate::strings(&["<your site>.atlassian.net"]),
        },
        ..crate::entry(
            "jira",
            "Jira",
            Size::Medium,
            sections_view(vec![("Assigned".into(), issues)]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results() {
        let json = serde_json::json!({ "issues": [{ "key": "OPS-7", "fields": {
            "summary": "Rotate certs", "status": { "name": "To Do" },
            "updated": "2026-10-27T04:09:57.000+0200", "priority": { "name": "High" }, "labels": ["infra"]
        }}]});
        let issues = parse(&json, "https://acme.atlassian.net").unwrap();
        assert_eq!(issues[0].url, "https://acme.atlassian.net/browse/OPS-7");
        assert_eq!(issues[0].tags, ["High", "infra"]);
        assert_eq!(issues[0].status, Some(Status::Warn));
    }
}
