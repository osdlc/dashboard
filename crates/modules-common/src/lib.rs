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
pub mod secret;
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

/// `~/x` as `$HOME/x`; other paths as they are. On Windows the home is
/// `%USERPROFILE%` when `HOME` is unset, and `~\x` works too.
pub fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|_| cfg!(windows)));
    expand_home_in(path, home.map(PathBuf::from), cfg!(windows))
}

fn expand_home_in(path: &str, home: Option<PathBuf>, windows: bool) -> PathBuf {
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\").filter(|_| windows));
    match (rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn expands_the_home_folder() {
        let home = || Some(PathBuf::from("/home/me"));
        assert_eq!(
            expand_home_in("~/x/y", home(), false),
            Path::new("/home/me/x/y")
        );
        assert_eq!(expand_home_in("/abs", home(), false), Path::new("/abs"));
        assert_eq!(expand_home_in("~/x", None, false), Path::new("~/x"));
        // `~\` only counts on Windows.
        assert_eq!(expand_home_in("~\\x", home(), false), Path::new("~\\x"));
        assert_eq!(
            expand_home_in("~\\x", home(), true),
            Path::new("/home/me").join("x")
        );
    }
}
