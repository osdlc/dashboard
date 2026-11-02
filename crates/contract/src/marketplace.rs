//! The marketplace index: a JSON file listing plugin packages that can be
//! installed, each pinned by URL and sha256. See docs/marketplace.md.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::{CatalogEntry, EntryKind, MARKETPLACE, Package, Requirements};
use crate::{Size, View};

/// The index format this dashboard reads and writes.
pub const INDEX_FORMAT: u32 = 1;

/// `index.json`: the packages a marketplace offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Index {
    /// The index format. Always 1 for now.
    pub format: u32,
    #[serde(default)]
    pub plugins: Vec<IndexEntry>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            format: INDEX_FORMAT,
            plugins: vec![],
        }
    }
}

impl Index {
    pub fn get(&self, id: &str) -> Option<&IndexEntry> {
        self.plugins.iter().find(|e| e.id == id)
    }
}

/// One package in the index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    /// The package's ID, as in its `dashboard-plugin.yaml`.
    pub id: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Where the `.tar.gz` is: an `https://` or `file://` URL, or a path
    /// relative to the index file. An index on the web may only use
    /// `https://`; plain `http://` is refused everywhere.
    pub url: String,
    /// Lowercase hex sha256 of the `.tar.gz`. Install refuses a file that
    /// doesn't match.
    pub sha256: String,
    /// The interpreter it needs, e.g. `node>=22` or `python3>=3.9`.
    #[serde(default)]
    pub runtime: Option<String>,
    /// Other programs, environment variables and hosts it uses.
    #[serde(default)]
    pub requirements: Requirements,
    /// The rest is what the web editor's catalog shows before the package is
    /// installed, copied from its `dashboard-plugin.yaml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo: Option<View>,
}

impl IndexEntry {
    /// The entry for `package`, packed at `url` with this hash.
    pub fn new(package: &Package, url: String, sha256: String) -> Self {
        Self {
            id: package.id.clone(),
            version: package.version.clone(),
            description: package.description.clone(),
            tags: package.tags.clone(),
            url,
            sha256,
            runtime: package.runtime.clone(),
            requirements: Requirements {
                binaries: package.binaries.clone(),
                env: package.env.clone(),
                hosts: package.hosts.clone(),
            },
            name: Some(package.name.clone()),
            size: package.size,
            config_schema: package.config_schema.clone(),
            demo: Some(package.demo.clone()),
        }
    }

    /// The catalog entry for this package, not yet installed. None when the
    /// index gives no demo view to show.
    pub fn entry(&self) -> Option<CatalogEntry> {
        Some(CatalogEntry {
            id: self.id.clone(),
            name: self.name.clone().unwrap_or_else(|| self.id.clone()),
            version: self.version.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
            kind: EntryKind::Plugin,
            source: MARKETPLACE.into(),
            default_size: self.size.unwrap_or(Size::Medium),
            config_schema: self.config_schema.clone(),
            requirements: Requirements {
                binaries: self
                    .runtime
                    .iter()
                    .chain(&self.requirements.binaries)
                    .cloned()
                    .collect(),
                env: self.requirements.env.clone(),
                hosts: self.requirements.hosts.clone(),
            },
            demo: self.demo.clone()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_index() {
        let index: Index = serde_json::from_value(serde_json::json!({
            "format": 1,
            "plugins": [{
                "id": "hello",
                "version": "0.1.0",
                "description": "A greeting.",
                "url": "packages/hello-0.1.0.tar.gz",
                "sha256": "00",
                "runtime": "python3>=3.9",
                "requirements": { "hosts": ["example.com"] }
            }]
        }))
        .unwrap();
        let e = index.get("hello").unwrap();
        assert_eq!(e.requirements.hosts, ["example.com"]);
        assert!(e.tags.is_empty());
        assert!(index.get("nope").is_none());
        // Unknown keys are a mistake, not something to ignore.
        let bad = serde_json::from_value::<Index>(serde_json::json!({ "format": 1, "plugin": [] }));
        assert!(bad.is_err());
    }

    #[test]
    fn index_entry_is_a_marketplace_plugin() {
        let index: Index = serde_json::from_value(serde_json::json!({
            "format": 1,
            "plugins": [{
                "id": "ci", "version": "1.0.0", "name": "CI", "description": "Runs",
                "runtime": "python3>=3.9", "size": "wide", "url": "https://x/ci.tar.gz",
                "sha256": "00", "requirements": { "binaries": ["gh"] },
                "demo": {"status": "ok", "summary": "", "root": {"type": "text", "text": "hi"}}
            }, {
                "id": "bare", "version": "1.0.0", "description": "No demo",
                "url": "bare.tar.gz", "sha256": "00"
            }]
        }))
        .unwrap();
        let e = index.plugins[0].entry().unwrap();
        assert_eq!(e.kind, EntryKind::Plugin);
        assert!(!e.installed());
        assert_eq!(e.default_size, Size::Wide);
        assert_eq!(e.requirements.binaries, ["python3>=3.9", "gh"]);
        assert!(index.plugins[1].entry().is_none());
    }
}
