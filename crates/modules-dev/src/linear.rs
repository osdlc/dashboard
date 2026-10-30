//! Linear issues assigned to (or created by) you, via the GraphQL API.
//!
//! ```yaml
//! modules:
//!   linear:
//!     config: { token_env: LINEAR_API_KEY, limit: 10, sections: [assigned, created] }
//! ```

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{CatalogEntry, Manifest, Requirements, Size, Status, View};
use dashboard_host::Module;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{Issue, Secret, check, client, demo_time, manifest, sections_view};

const API: &str = "https://api.linear.app/graphql";

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Section {
    Assigned,
    Created,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    #[serde(flatten)]
    secret: Secret,
    limit: u32,
    sections: Vec<Section>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            secret: Secret::default(),
            limit: 10,
            sections: vec![Section::Assigned],
        }
    }
}

#[derive(Default)]
pub struct Linear {
    config: Config,
}

#[async_trait]
impl Module for Linear {
    async fn init(&mut self, config: serde_json::Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        Ok(manifest("linear", "Linear"))
    }

    async fn render(&mut self) -> Result<View> {
        let token = self.config.secret.token("LINEAR_API_KEY").await?;
        let http = client()?;
        let mut sections = vec![];
        for section in &self.config.sections {
            let (label, field) = match section {
                Section::Assigned => ("Assigned", "assignedIssues"),
                Section::Created => ("Created by me", "createdIssues"),
            };
            let body = serde_json::json!({ "query": query(field), "variables": { "first": self.config.limit } });
            // Personal API keys go in the header as-is, without "Bearer".
            let resp = http
                .post(API)
                .header("Authorization", &token)
                .json(&body)
                .send()
                .await?;
            let json: serde_json::Value = check(resp).await?.json().await?;
            sections.push((label.to_string(), parse(&json, field)?));
        }
        Ok(sections_view(sections))
    }
}

fn query(field: &str) -> String {
    format!(
        "query($first: Int!) {{ viewer {{ {field}(first: $first, orderBy: updatedAt, \
         filter: {{ state: {{ type: {{ nin: [\"completed\", \"canceled\"] }} }} }}) {{ nodes {{ \
         identifier title url updatedAt priority priorityLabel state {{ name }} labels {{ nodes {{ name }} }} }} }} }} }}"
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    identifier: String,
    title: String,
    url: String,
    updated_at: Option<String>,
    /// 0 none, 1 urgent, 2 high, 3 medium, 4 low.
    #[serde(default)]
    priority: u8,
    priority_label: Option<String>,
    state: Option<Named>,
    labels: Option<Nodes<Named>>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

fn parse(json: &serde_json::Value, field: &str) -> Result<Vec<Issue>> {
    if let Some(errors) = json.get("errors").and_then(|e| e.as_array()) {
        let msgs: Vec<&str> = errors
            .iter()
            .filter_map(|e| e["message"].as_str())
            .collect();
        bail!("Linear: {}", msgs.join("; "));
    }
    let nodes: Nodes<Node> = serde_json::from_value(json["data"]["viewer"][field].clone())?;
    Ok(nodes
        .nodes
        .into_iter()
        .map(|n| {
            let mut tags: Vec<String> = n
                .labels
                .map(|l| l.nodes.into_iter().map(|x| x.name).collect())
                .unwrap_or_default();
            if n.priority != 0 {
                tags.insert(0, n.priority_label.unwrap_or_default());
            }
            Issue {
                key: n.identifier,
                title: n.title,
                url: n.url,
                state: n.state.map(|s| s.name),
                updated: n.updated_at,
                tags,
                status: match n.priority {
                    1 => Some(Status::Error),
                    2 => Some(Status::Warn),
                    _ => None,
                },
            }
        })
        .collect())
}

pub fn catalog() -> CatalogEntry {
    // A hand-written GraphQL reply in the shape of the query above.
    let node = |id: &str, title: &str, state: &str, hours_ago: i64, priority: u8, label: &str| {
        let priority_label = ["No priority", "Urgent", "High", "Medium", "Low"][priority as usize];
        serde_json::json!({
            "identifier": id, "title": title, "url": format!("https://linear.app/acme/issue/{id}"),
            "updatedAt": demo_time(hours_ago * 3600), "priority": priority, "priorityLabel": priority_label,
            "state": { "name": state }, "labels": { "nodes": [{ "name": label }] }
        })
    };
    let reply = serde_json::json!({ "data": { "viewer": { "assignedIssues": { "nodes": [
        node("ENG-482", "Search results flicker while typing", "In Progress", 1, 1, "bug"),
        node("ENG-470", "Cache avatar images on the CDN", "Todo", 20, 3, "performance"),
        node("ENG-455", "Onboarding checklist for new workspaces", "Backlog", 74, 4, "feature"),
    ]}}}});
    let issues = parse(&reply, "assignedIssues").expect("demo data parses");
    CatalogEntry {
        description: "Linear issues assigned to you or created by you. Untested live: built from \
                      the API docs with sample data."
            .into(),
        tags: crate::strings(&["issues", "dev", "untested-live"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: crate::strings(&["LINEAR_API_KEY"]),
            hosts: crate::strings(&["api.linear.app"]),
        },
        ..crate::entry(
            "linear",
            "Linear",
            Size::Medium,
            sections_view(vec![("Assigned".into(), issues)]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_issues_and_errors() {
        let json = serde_json::json!({ "data": { "viewer": { "assignedIssues": { "nodes": [{
            "identifier": "ENG-12", "title": "Fix login", "url": "https://linear.app/x/issue/ENG-12",
            "updatedAt": "2026-10-27T02:09:57.123Z", "priority": 1, "priorityLabel": "Urgent",
            "state": { "name": "In Progress" }, "labels": { "nodes": [{ "name": "bug" }] }
        }]}}}});
        let issues = parse(&json, "assignedIssues").unwrap();
        assert_eq!(issues[0].key, "ENG-12");
        assert_eq!(issues[0].tags, ["Urgent", "bug"]);
        assert_eq!(issues[0].status, Some(Status::Error));

        let err = serde_json::json!({ "errors": [{ "message": "Authentication required" }] });
        assert!(
            parse(&err, "assignedIssues")
                .unwrap_err()
                .to_string()
                .contains("Authentication")
        );
    }
}
