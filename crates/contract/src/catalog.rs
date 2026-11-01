//! The catalog: everything that can be added to a dashboard, built in or
//! installed, with what it needs and a demo view that works with no account.
//! Also the plugin package file (`dashboard-plugin.yaml`). See docs/plugins.md.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Size, View};

/// The file at the root of a plugin package.
pub const PACKAGE_FILE: &str = "dashboard-plugin.yaml";

/// One thing the catalog lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatalogEntry {
    /// The module ID to put in a config: under `modules:` for a built-in, or
    /// `package:` for a plugin package.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub kind: EntryKind,
    /// Where it comes from: `builtin`, `bundled` (a plugin inside the
    /// dashboard binary), or the folder of an installed package.
    pub source: String,
    pub default_size: Size,
    /// JSON Schema for the module's `config:` block.
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub requirements: Requirements,
    /// Made-up but realistic data, shown in previews. Needs no account.
    pub demo: View,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// Compiled into the dashboard and run in its process.
    Builtin,
    /// A separate program run over stdio.
    Plugin,
}

/// What a module needs to work. Shown before adding or installing it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Requirements {
    /// Programs on PATH, with an optional minimum version, e.g. `gh`, `node>=22`.
    #[serde(default)]
    pub binaries: Vec<String>,
    /// Environment variables it reads, e.g. API tokens.
    #[serde(default)]
    pub env: Vec<String>,
    /// Network hosts it calls.
    #[serde(default)]
    pub hosts: Vec<String>,
}

impl CatalogEntry {
    /// True when every word of `query` is in the name, ID, description or a
    /// tag (case-insensitive), and the entry has `tag` if one is given.
    pub fn matches(&self, query: &str, tag: Option<&str>) -> bool {
        if let Some(tag) = tag
            && !self.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
        {
            return false;
        }
        let haystack = format!(
            "{} {} {} {}",
            self.id,
            self.name,
            self.description,
            self.tags.join(" ")
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_lowercase()))
    }
}

/// The `source` of a marketplace entry: listed in the cached index but not
/// installed, so it can't be added yet.
pub const MARKETPLACE: &str = "marketplace";

impl CatalogEntry {
    /// False for a marketplace entry that isn't installed.
    pub fn installed(&self) -> bool {
        self.source != MARKETPLACE
    }
}

/// What `GET /api/catalog` returns to the web editor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatalogResponse {
    /// Entries matching the request's `q` and `tag`.
    pub entries: Vec<CatalogEntry>,
    /// Every tag in the whole catalog, sorted, for the filter buttons.
    pub tags: Vec<String>,
    /// Problems reading packages or the marketplace index, to show.
    #[serde(default)]
    pub errors: Vec<String>,
}

impl CatalogResponse {
    /// Search `all` for `query` and `tag`, and list every tag.
    pub fn new(all: &[CatalogEntry], query: &str, tag: Option<&str>, errors: Vec<String>) -> Self {
        let mut tags: Vec<String> = all.iter().flat_map(|e| e.tags.iter().cloned()).collect();
        tags.sort();
        tags.dedup();
        Self {
            entries: search(all, query, tag).into_iter().cloned().collect(),
            tags,
            errors,
        }
    }
}

/// Entries matching `query` and `tag`, in the order given.
pub fn search<'a>(
    entries: &'a [CatalogEntry],
    query: &str,
    tag: Option<&str>,
) -> Vec<&'a CatalogEntry> {
    entries.iter().filter(|e| e.matches(query, tag)).collect()
}

/// A plugin package: a folder holding this file (`dashboard-plugin.yaml`)
/// and the plugin's own files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// Lowercase letters, digits and dashes. Also the package's folder name.
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Program and arguments, run in the package's folder. A program given
    /// as a path (`./plugin`, `bin/plugin`) is relative to the folder too.
    pub run: Vec<String>,
    /// The interpreter it needs, e.g. `node>=22` or `python3>=3.9`.
    #[serde(default)]
    pub runtime: Option<String>,
    /// Other programs it runs, e.g. `gh` or `git`.
    #[serde(default)]
    pub binaries: Vec<String>,
    /// Environment variables it reads.
    #[serde(default)]
    pub env: Vec<String>,
    /// Network hosts it calls.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Suggested tile size. Default `medium`.
    #[serde(default)]
    pub size: Option<Size>,
    /// JSON Schema for the tile's `config:` block.
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
    /// A view with made-up data, shown in the catalog before it is set up.
    pub demo: View,
}

impl Package {
    /// The catalog entry for this package, installed in `dir`.
    pub fn entry(&self, dir: &str) -> CatalogEntry {
        CatalogEntry {
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
            kind: EntryKind::Plugin,
            source: dir.into(),
            default_size: self.size.unwrap_or(Size::Medium),
            config_schema: self.config_schema.clone(),
            requirements: Requirements {
                binaries: self.runtime.iter().chain(&self.binaries).cloned().collect(),
                env: self.env.clone(),
                hosts: self.hosts.clone(),
            },
            demo: self.demo.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Status, Widget};

    fn entry(id: &str, name: &str, description: &str, tags: &[&str]) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            name: name.into(),
            version: "0.1.0".into(),
            description: description.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            kind: EntryKind::Builtin,
            source: "builtin".into(),
            default_size: Size::Medium,
            config_schema: None,
            requirements: Requirements::default(),
            demo: View {
                status: Status::Ok,
                summary: String::new(),
                root: Widget::Text {
                    text: String::new(),
                    markdown: false,
                },
            },
        }
    }

    #[test]
    fn search_is_case_insensitive_over_name_description_and_tags() {
        let all = vec![
            entry("cpu", "CPU", "Usage per core", &["system"]),
            entry(
                "github-prs",
                "Pull requests",
                "From the gh CLI",
                &["github", "dev"],
            ),
            entry("jira", "Jira", "Issues from JQL", &["issues", "dev"]),
        ];
        let ids =
            |found: Vec<&CatalogEntry>| found.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(search(&all, "", None)).len(), 3);
        assert_eq!(ids(search(&all, "PULL", None)), ["github-prs"]);
        assert_eq!(ids(search(&all, "core", None)), ["cpu"]);
        assert_eq!(ids(search(&all, "github", None)), ["github-prs"]);
        // Every word must match, anywhere.
        assert_eq!(ids(search(&all, "issues jql", None)), ["jira"]);
        assert!(search(&all, "issues docker", None).is_empty());
        // Tags filter exactly, ignoring case.
        assert_eq!(ids(search(&all, "", Some("Dev"))), ["github-prs", "jira"]);
        assert_eq!(ids(search(&all, "jira", Some("dev"))), ["jira"]);
        assert!(search(&all, "", Some("de")).is_empty());
    }

    #[test]
    fn package_entry_lists_the_runtime_first() {
        let p: Package = serde_json::from_value(serde_json::json!({
            "id": "ci", "version": "1.0.0", "name": "CI", "description": "Runs",
            "run": ["node", "ci.mjs"], "runtime": "node>=22", "binaries": ["gh"],
            "demo": {"status": "ok", "summary": "", "root": {"type": "text", "text": "hi"}}
        }))
        .unwrap();
        let e = p.entry("/plugins/ci");
        assert_eq!(e.kind, EntryKind::Plugin);
        assert_eq!(e.default_size, Size::Medium);
        assert_eq!(e.requirements.binaries, ["node>=22", "gh"]);
        assert_eq!(e.source, "/plugins/ci");
    }

    #[test]
    fn response_lists_every_tag_but_only_matching_entries() {
        let all = vec![
            entry("cpu", "CPU", "Usage per core", &["system"]),
            entry("jira", "Jira", "Issues", &["issues", "dev"]),
        ];
        let r = CatalogResponse::new(&all, "", Some("dev"), vec![]);
        assert_eq!(r.tags, ["dev", "issues", "system"]);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].id, "jira");
    }
}
