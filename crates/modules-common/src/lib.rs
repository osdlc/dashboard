//! What the built-in module bundles share: the registration each module
//! makes, catalog entry helpers, manifests, times and (with the `http`
//! feature) HTTP helpers.

use std::path::PathBuf;

use dashboard_contract::{
    CONTRACT_VERSION, CatalogEntry, EntryKind, LayoutHints, Manifest, Refresh, Requirements, Size,
    View,
};
use dashboard_host::Module;
use schemars::JsonSchema;

#[cfg(feature = "http")]
pub mod http;
pub mod time;

/// One built-in module, as a bundle lists it.
pub struct Builtin {
    pub id: &'static str,
    /// Listed before the other built-ins: the system and GitHub tiles.
    pub core: bool,
    /// A fresh instance.
    pub new: fn() -> Box<dyn Module>,
    pub catalog: fn() -> CatalogEntry,
}

/// The parts every built-in entry shares. Fill in the rest with
/// `CatalogEntry { description, tags, .., ..entry(...) }`.
pub fn entry(id: &str, name: &str, default_size: Size, demo: View) -> CatalogEntry {
    CatalogEntry {
        id: id.into(),
        name: name.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: String::new(),
        tags: vec![],
        kind: EntryKind::Builtin,
        source: "builtin".into(),
        default_size,
        config_schema: None,
        requirements: Requirements::default(),
        demo,
    }
}

/// JSON Schema for a module's `config:` block.
pub fn schema<T: JsonSchema>() -> Option<serde_json::Value> {
    serde_json::to_value(schemars::schema_for!(T)).ok()
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub fn manifest(
    id: &str,
    name: &str,
    interval_ms: u64,
    min_interval_ms: u64,
    size: Size,
    priority: i32,
) -> Manifest {
    Manifest {
        id: id.into(),
        name: name.into(),
        contract_version: CONTRACT_VERSION.into(),
        refresh: Refresh::Poll {
            interval_ms,
            min_interval_ms: Some(min_interval_ms),
        },
        layout: LayoutHints {
            size,
            priority,
            min_size: Some(Size::Small),
        },
        commands: vec![],
        config_schema: None,
    }
}

/// `~/x` as `$HOME/x`; other paths as they are.
pub fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}
