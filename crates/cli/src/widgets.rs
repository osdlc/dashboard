//! `dashboard widgets`: search the catalog of everything that can be added.

use std::path::Path;

use anyhow::Result;
use dashboard_contract::catalog::search;
use dashboard_contract::{CatalogEntry, EntryKind};

pub fn run(config_path: &Path, query: &str, tag: Option<&str>, json: bool) -> Result<()> {
    let (entries, errors) = crate::runtime::catalog(config_path);
    for err in &errors {
        eprintln!("skipped a plugin package: {err}");
    }
    let found = search(&entries, query, tag);
    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }
    if found.is_empty() {
        println!("nothing matches");
        return Ok(());
    }
    print!("{}", table(&found));
    Ok(())
}

/// One line per entry (ID, kind, name, tags), then its description indented.
fn table(entries: &[&CatalogEntry]) -> String {
    let id_width = entries.iter().map(|e| e.id.len()).max().unwrap_or(0);
    let name_width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    for e in entries {
        let kind = match e.kind {
            EntryKind::Builtin => "builtin",
            EntryKind::Plugin => "plugin",
        };
        let line = format!(
            "{:id_width$}  {kind:7}  {:name_width$}  {}",
            e.id,
            e.name,
            e.tags.join(", ")
        );
        out.push_str(line.trim_end());
        out.push('\n');
        out.push_str(&format!("    {}\n", e.description));
    }
    out
}

#[cfg(all(test, feature = "module-github-prs"))]
mod tests {
    use super::*;

    #[test]
    fn table_lists_matches_with_descriptions() {
        // Built-ins only: the full catalog also reads packages installed on this machine.
        let entries = dashboard_modules::catalog();
        let found = search(&entries, "requests", Some("github"));
        let text = table(&found);
        assert!(
            text.starts_with("github-prs  builtin  Pull requests"),
            "{text}"
        );
        assert!(text.contains("\n    Open pull requests"), "{text}");
        assert!(!text.contains("cpu"), "{text}");
    }
}
