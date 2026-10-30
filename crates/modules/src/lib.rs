//! Built-in modules. They live in bundle crates (system, dev, fun, agents),
//! each behind cargo features; this crate gathers whatever the build includes.
//! See docs/modules.md.

use dashboard_contract::CatalogEntry;
use dashboard_host::Module;
pub use dashboard_modules_common::Builtin;

/// Each included bundle's modules, in bundle order.
fn bundles() -> Vec<&'static [Builtin]> {
    vec![
        #[cfg(feature = "dashboard-modules-system")]
        dashboard_modules_system::MODULES,
        #[cfg(feature = "dashboard-modules-dev")]
        dashboard_modules_dev::MODULES,
        #[cfg(feature = "dashboard-modules-fun")]
        dashboard_modules_fun::MODULES,
        #[cfg(feature = "dashboard-modules-agents")]
        dashboard_modules_agents::MODULES,
    ]
}

/// Every built-in module in this build, in display order: the core ones
/// (system and GitHub) first, then the rest.
pub fn all() -> Vec<&'static Builtin> {
    let mut all: Vec<&Builtin> = bundles().into_iter().flatten().collect();
    all.sort_by_key(|b| !b.core);
    all
}

/// IDs of every built-in module in this build, in display order.
pub fn ids() -> Vec<&'static str> {
    all().into_iter().map(|b| b.id).collect()
}

/// A fresh instance of a built-in module, if this build includes it.
pub fn builtin(id: &str) -> Option<Box<dyn Module>> {
    all().into_iter().find(|b| b.id == id).map(|b| (b.new)())
}

/// Catalog entries for every built-in module in this build, in display order.
pub fn catalog() -> Vec<CatalogEntry> {
    all().into_iter().map(|b| (b.catalog)()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::check::check_view;

    #[test]
    fn every_builtin_has_a_matching_catalog_entry() {
        let ids: Vec<String> = catalog().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, super::ids());
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "duplicate ids: {ids:?}");
    }

    #[cfg(all(
        feature = "bundle-system",
        feature = "bundle-dev",
        feature = "bundle-fun",
        feature = "bundle-agents"
    ))]
    #[test]
    fn default_build_has_every_module() {
        assert_eq!(
            ids(),
            [
                "cpu",
                "memory",
                "network",
                "disk",
                "processes",
                "github-prs",
                "github-issues",
                "log",
                "uptime",
                "screensaver",
                "jira",
                "linear",
                "forgejo",
                "demo",
                "codex",
                "copilot",
            ]
        );
    }

    #[cfg(all(
        feature = "bundle-system",
        not(feature = "bundle-dev"),
        not(feature = "bundle-fun"),
        not(feature = "bundle-agents")
    ))]
    #[test]
    fn system_only_build_has_only_system_modules() {
        assert_eq!(
            ids(),
            [
                "cpu",
                "memory",
                "network",
                "disk",
                "processes",
                "log",
                "uptime",
                "screensaver"
            ]
        );
        assert!(builtin("jira").is_none());
        assert!(builtin("cpu").is_some());
    }

    #[test]
    fn catalog_entries_are_complete_and_demos_are_valid() {
        for e in catalog() {
            assert!(builtin(&e.id).is_some(), "{}: no module", e.id);
            assert!(!e.description.is_empty(), "{}: no description", e.id);
            assert!(!e.tags.is_empty(), "{}: no tags", e.id);
            if let Some(schema) = &e.config_schema {
                assert_eq!(schema["type"], "object", "{}", e.id);
            }
            let problems: Vec<String> = check_view(&e.demo)
                .into_iter()
                .filter(|f| f.problem)
                .map(|f| f.text)
                .collect();
            assert!(problems.is_empty(), "{}: {problems:?}", e.id);
            // Survives the trip through JSON, as the web view gets it.
            let json = serde_json::to_value(&e).unwrap();
            let back: CatalogEntry = serde_json::from_value(json).unwrap();
            assert_eq!(back, e);
        }
    }
}
