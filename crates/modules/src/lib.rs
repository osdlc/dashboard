//! Built-in modules. They live in bundle crates (system, dev, ops, fun, agents,
//! personal), each behind cargo features; this crate gathers whatever the build
//! includes. See docs/modules.md.

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
        #[cfg(feature = "dashboard-modules-ops")]
        dashboard_modules_ops::MODULES,
        #[cfg(feature = "dashboard-modules-fun")]
        dashboard_modules_fun::MODULES,
        #[cfg(feature = "dashboard-modules-agents")]
        dashboard_modules_agents::MODULES,
        #[cfg(feature = "dashboard-modules-personal")]
        dashboard_modules_personal::MODULES,
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

/// Make the relative paths in a built-in's `config:` start from `dir`, the
/// folder of the config file that set it. Which settings are paths comes
/// from the module's config schema (see `dashboard_modules_common::PATH_KEYWORD`).
pub fn resolve_paths(id: &str, config: &mut serde_json::Value, dir: &std::path::Path) {
    if let Some(schema) = all()
        .into_iter()
        .find(|b| b.id == id)
        .and_then(|b| (b.catalog)().config_schema)
    {
        dashboard_modules_common::resolve_paths(config, &schema, dir);
    }
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
        feature = "bundle-ops",
        feature = "bundle-fun",
        feature = "bundle-agents",
        feature = "bundle-personal"
    ))]
    #[test]
    fn default_build_has_every_module() {
        assert_eq!(
            ids(),
            [
                "cpu",
                "memory",
                "gpu",
                "network",
                "disk",
                "processes",
                "github-prs",
                "github-issues",
                "log",
                "uptime",
                "textfile",
                "battery",
                "cmdrunner",
                "security",
                "http",
                "file",
                "jira",
                "linear",
                "forgejo",
                "git",
                "docker",
                "kubernetes",
                "ipinfo",
                "pihole",
                "transmission",
                "demo",
                "clock",
                "hackernews",
                "feedreader",
                "subreddit",
                "devto",
                "mempool",
                "crypto",
                "scores",
                "codex",
                "copilot",
                "cursor",
                "antigravity",
                "gemini",
                "opencode",
                "pi",
                "weather",
                "todo",
                "lunar",
                "calendar",
                "org-agenda",
                "markdown-tasks",
                "todo-txt",
                "taskwarrior",
                "sdlc",
            ]
        );
    }

    #[cfg(all(
        feature = "bundle-system",
        not(feature = "bundle-dev"),
        not(feature = "bundle-ops"),
        not(feature = "bundle-fun"),
        not(feature = "bundle-agents"),
        not(feature = "bundle-personal")
    ))]
    #[test]
    fn system_only_build_has_only_system_modules() {
        assert_eq!(
            ids(),
            [
                "cpu",
                "memory",
                "gpu",
                "network",
                "disk",
                "processes",
                "log",
                "uptime",
                "textfile",
                "battery",
                "cmdrunner",
                "security",
                "http",
                "file"
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

    /// A module's schema is described by the module, not by a helper type
    /// it flattens in (such as `Secret`, for the token settings).
    #[test]
    fn schemas_are_not_described_by_a_helper() {
        let helpers: Vec<serde_json::Value> = dashboard_modules_common::flattened_helpers()
            .iter()
            .filter_map(|h| h.get("description").cloned())
            .collect();
        assert!(!helpers.is_empty());
        for e in catalog() {
            if let Some(d) = e.config_schema.as_ref().and_then(|s| s.get("description")) {
                assert!(!helpers.contains(d), "{}: {d}", e.id);
            }
        }
    }

    /// Settings with a path-like name are marked as paths, so the runtime
    /// resolves them from the config file's folder. A new module's `path:`
    /// that isn't marked would read from the working directory instead.
    #[test]
    fn path_settings_are_marked() {
        const NAMES: &[&str] = &[
            "path", "paths", "repos", "sample", "home", "db", "feeds", "file", "dir",
        ];
        for e in catalog() {
            let Some(props) = e
                .config_schema
                .as_ref()
                .and_then(|s| s["properties"].as_object())
            else {
                continue;
            };
            for (key, prop) in props {
                let is_path = NAMES.contains(&key.as_str());
                assert_eq!(
                    prop.get(dashboard_modules_common::PATH_KEYWORD)
                        == Some(&serde_json::Value::Bool(true)),
                    is_path,
                    "{}.{key}",
                    e.id
                );
            }
        }
    }

    #[cfg(all(feature = "bundle-system", feature = "bundle-dev"))]
    #[test]
    fn relative_paths_start_in_the_given_folder() {
        let dir = std::env::temp_dir();
        let mut textfile = serde_json::json!({ "path": "TODO.md", "lines": 5 });
        resolve_paths("textfile", &mut textfile, &dir);
        assert_eq!(
            textfile["path"],
            dir.join("TODO.md").to_string_lossy().as_ref()
        );
        assert_eq!(textfile["lines"], 5);
        let mut git = serde_json::json!({ "repos": [".", "~/code/x"] });
        resolve_paths("git", &mut git, &dir);
        assert_eq!(
            git["repos"],
            serde_json::json!([dir.to_string_lossy(), "~/code/x"])
        );
    }
}
