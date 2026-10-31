//! What the built-in module bundles share: the registration each module
//! makes, catalog entry helpers, manifests, times and (with the `http`
//! feature) HTTP helpers.

use std::path::{Component, Path, PathBuf};

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
    let mut schema = serde_json::to_value(schemars::schema_for!(T)).ok()?;
    // A flattened helper's description lands on the module's schema when the
    // module's `Config` has none of its own; it describes the helper, not the
    // module.
    let helpers: Vec<serde_json::Value> = flattened_helpers()
        .iter()
        .filter_map(|h| h.get("description").cloned())
        .collect();
    if let Some(obj) = schema.as_object_mut()
        && obj.get("description").is_some_and(|d| helpers.contains(d))
    {
        obj.remove("description");
    }
    Some(schema)
}

/// Schemas of the types modules flatten into their `Config`
/// (`#[serde(flatten)]`). Add any new one here.
pub fn flattened_helpers() -> Vec<serde_json::Value> {
    [serde_json::to_value(schemars::schema_for!(secret::Secret))]
        .into_iter()
        .filter_map(Result::ok)
        .collect()
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

/// The schema keyword that marks a setting as a file or folder path: a
/// string, or a list of strings. Put it on the field with
/// `#[schemars(extend("x-path" = true))]`. Before a built-in starts, the
/// runtime makes the relative ones start from the folder of the config file
/// that set them (see `resolve_paths`).
pub const PATH_KEYWORD: &str = "x-path";

/// Make the relative paths in `config` start from `dir`: every top-level
/// setting that `schema` marks with `PATH_KEYWORD`. Absolute paths, `~/`
/// paths and URLs stay as they are. An empty `dir` changes nothing.
pub fn resolve_paths(config: &mut serde_json::Value, schema: &serde_json::Value, dir: &Path) {
    use serde_json::Value;
    if dir.as_os_str().is_empty() {
        return;
    }
    let (Some(config), Some(Value::Object(props))) =
        (config.as_object_mut(), schema.get("properties"))
    else {
        return;
    };
    for (key, prop) in props {
        if prop.get(PATH_KEYWORD) != Some(&Value::Bool(true)) {
            continue;
        }
        match config.get_mut(key) {
            Some(Value::String(s)) => resolve_path(s, dir),
            Some(Value::Array(items)) => {
                for item in items {
                    if let Value::String(s) = item {
                        resolve_path(s, dir);
                    }
                }
            }
            _ => {}
        }
    }
}

fn resolve_path(path: &mut String, dir: &Path) {
    if path.is_empty()
        || path.starts_with('~')
        || path.contains("://")
        || Path::new(path.as_str()).is_absolute()
    {
        return;
    }
    let mut out = dir.to_path_buf();
    for part in Path::new(path.as_str()).components() {
        if part != Component::CurDir {
            out.push(part);
        }
    }
    *path = out.to_string_lossy().into_owned();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_marked_relative_paths_from_the_folder() {
        let schema = json!({ "properties": {
            "path": { "type": "string", "x-path": true },
            "repos": { "type": "array", "x-path": true },
            "name": { "type": "string" },
        }});
        let tmp = std::env::temp_dir();
        let dir = tmp.as_path();
        let abs = dir.join("abs").to_string_lossy().into_owned();
        let mut config = json!({
            "path": "notes/TODO.md",
            "repos": [".", "../other", abs, "~/code/x", "https://example.com/feed"],
            "name": "TODO.md",
        });
        resolve_paths(&mut config, &schema, dir);
        assert_eq!(
            config["path"],
            json!(dir.join("notes").join("TODO.md").to_string_lossy())
        );
        assert_eq!(
            config["repos"],
            json!([
                dir.to_string_lossy(),
                dir.join("..").join("other").to_string_lossy(),
                abs,
                "~/code/x",
                "https://example.com/feed",
            ])
        );
        // Not marked: left alone.
        assert_eq!(config["name"], "TODO.md");

        // No folder: nothing changes.
        let mut config = json!({ "path": "TODO.md" });
        resolve_paths(&mut config, &schema, Path::new(""));
        assert_eq!(config["path"], "TODO.md");
    }

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
