//! Adding a module to the config file: its `modules:` entry and a tile on a
//! screen, written in one go.
//!
//! yaml-splice edits top-level keys only. To edit inside `modules:` or a
//! screen, the block is cut out, its indent removed, edited as a document of
//! its own with yaml-splice, then indented and put back. Comments and layout
//! inside the block stay. When the block is in flow style (`modules: {}`) or
//! empty, the whole key is written again instead.

use std::path::Path;

use anyhow::{Context, Result, bail};
use dashboard_contract::Size;
use yaml_splice::parse::parse_with_spans;
use yaml_splice::{FieldEdit, Mapping, Value, edit_yaml};

use crate::{parse, read_source, write_over};

/// A `modules:` entry to add.
#[derive(Debug, Clone, PartialEq)]
pub struct NewModule {
    /// The key under `modules:`, also the tile's `module:`.
    pub id: String,
    /// Run this built-in under `id` (when `id` is not the built-in's own).
    pub builtin: Option<String>,
    /// Run this installed package.
    pub package: Option<String>,
    pub demo: bool,
    /// Written on the module, for configs without `screens:`.
    pub size: Option<Size>,
    /// The module's `config:` block. Null or `{}` writes none.
    pub config: serde_json::Value,
}

/// Add `module` under `modules:` and, with `tile`, a tile at the end of that
/// screen (by name). Fails if the ID is already under `modules:`.
pub fn add_module(path: &Path, module: &NewModule, tile: Option<(&str, Size)>) -> Result<()> {
    if !valid_id(&module.id) {
        bail!(
            "module ID {:?}: use letters, digits, `-` and `_`, starting with a letter",
            module.id
        );
    }
    let (bytes, missing) = read_source(path)?;
    let source = String::from_utf8(bytes.clone())
        .with_context(|| format!("{} is not UTF-8", path.display()))?;
    let mut out = insert_module(&source, &module.id, module_value(module))?;
    if let Some((screen, size)) = tile {
        out = insert_tile(&out, screen, &module.id, size)?;
    }
    // Check the result reads back as meant before writing it.
    let config = parse(&out).context("the edited config doesn't parse")?;
    if !config.modules.contains_key(&module.id) {
        bail!("the edited config lost modules.{}", module.id);
    }
    if let Some((screen, _)) = tile {
        let placed = config
            .screens
            .iter()
            .any(|s| s.name == screen && s.tiles.iter().any(|t| t.module == module.id));
        if !placed {
            bail!(
                "the edited config has no tile for {:?} on {screen:?}",
                module.id
            );
        }
    }
    write_over(path, &bytes, missing, out.as_bytes())
}

/// Letters, digits, `-` and `_`, starting with a letter, and not a word YAML
/// reads as something other than a string.
fn valid_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_alphabetic())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !["true", "false", "null"]
            .iter()
            .any(|w| id.eq_ignore_ascii_case(w))
}

fn module_value(m: &NewModule) -> Value {
    let mut map = Mapping::new();
    if let Some(b) = &m.builtin {
        map.insert("builtin", Value::from(b.as_str()));
    }
    if let Some(p) = &m.package {
        map.insert("package", Value::from(p.as_str()));
    }
    if m.demo {
        map.insert("demo", Value::Bool(true));
    }
    if let Some(size) = m.size {
        map.insert("size", Value::from(size_name(size)));
    }
    let empty = m.config.is_null() || m.config.as_object().is_some_and(|o| o.is_empty());
    if !empty {
        map.insert("config", Value::from(&m.config));
    }
    Value::Map(map)
}

fn size_name(size: Size) -> &'static str {
    match size {
        Size::Small => "small",
        Size::Medium => "medium",
        Size::Large => "large",
        Size::Wide => "wide",
        Size::Tall => "tall",
    }
}

fn tile_value(id: &str, size: Size) -> Value {
    let mut map = Mapping::new();
    map.insert("module", Value::from(id));
    map.insert("size", Value::from(size_name(size)));
    Value::Map(map)
}

/// `src` with `id: value` added under `modules:`.
fn insert_module(src: &str, id: &str, value: Value) -> Result<String> {
    let parsed = parse_with_spans(src).map_err(|e| anyhow::anyhow!("{e}"))?;
    let existing = parsed.root.as_mapping().and_then(|m| m.get("modules"));
    let mut modules = match existing {
        None | Some(Value::Null) => Mapping::new(),
        Some(Value::Map(m)) => m.clone(),
        Some(_) => bail!("modules: must be a mapping"),
    };
    if modules.contains_key(id) {
        bail!("modules.{id} is already in the config");
    }
    let span = parsed.top_level.iter().find(|s| s.key == "modules");
    if let Some(block) = span.and_then(|s| block(src, s.value_range.clone()))
        && !modules.is_empty()
    {
        let sub = dedent(&src[block.start..block.end], block.indent);
        let out = set(&sub, id, value)?;
        return Ok(splice(src, &block, &indent_text(&out, block.indent)));
    }
    modules.insert(id, value);
    set(src, "modules", Value::Map(modules))
}

/// `src` with a tile for `id` added at the end of screen `screen`.
fn insert_tile(src: &str, screen: &str, id: &str, size: Size) -> Result<String> {
    let parsed = parse_with_spans(src).map_err(|e| anyhow::anyhow!("{e}"))?;
    let screens = parsed
        .root
        .as_mapping()
        .and_then(|m| m.get("screens"))
        .and_then(Value::as_seq)
        .unwrap_or_default();
    let Some(index) = screens.iter().position(|s| {
        s.as_mapping()
            .and_then(|m| m.get("name"))
            .and_then(Value::as_str)
            == Some(screen)
    }) else {
        bail!("no screen named {screen:?} in the config");
    };
    let tile = tile_value(id, size);

    // In place when `screens:` is a block list whose items we can find.
    let span = parsed.top_level.iter().find(|s| s.key == "screens");
    if let Some(block) = span.and_then(|s| block(src, s.value_range.clone())) {
        let items = items(&src[block.start..block.end], block.indent);
        if items.len() == screens.len()
            && let Some(range) = items.get(index)
        {
            let start = block.start + range.start;
            let end = block.start + range.end;
            if let Some(item) = add_to_item(&src[start..end], block.indent, tile.clone()) {
                let mut out = String::with_capacity(src.len() + item.len());
                out.push_str(&src[..start]);
                out.push_str(&item);
                out.push_str(&src[end..]);
                return Ok(out);
            }
        }
    }

    // Otherwise write `screens:` again; comments inside it are lost.
    let mut list = screens.to_vec();
    let Some(Value::Map(item)) = list.get_mut(index) else {
        bail!("screen {screen:?} is not a mapping");
    };
    let mut tiles = item
        .get("tiles")
        .and_then(Value::as_seq)
        .unwrap_or_default()
        .to_vec();
    tiles.push(tile);
    item.insert("tiles", Value::Seq(tiles));
    set(src, "screens", Value::Seq(list))
}

/// One screen's text (`- name: ...` and the lines under it) with `tile`
/// appended to its `tiles:`. `None` when its shape is unusual.
fn add_to_item(item: &str, indent: usize, tile: Value) -> Option<String> {
    let first = item.lines().next()?;
    let rest = first.get(indent..)?.strip_prefix("- ")?;
    if rest.starts_with([' ', '#']) || rest.trim().is_empty() {
        return None;
    }
    // Turn `- name: a` into `  name: a`, so the item is a plain mapping.
    let inner = indent + 2;
    let mut body = String::with_capacity(item.len());
    body.push_str(&item[..indent]);
    body.push_str("  ");
    body.push_str(&item[indent + 2..]);
    let sub = dedent(&body, inner);

    let parsed = parse_with_spans(&sub).ok()?;
    let span = parsed.top_level.iter().find(|s| s.key == "tiles")?;
    let tiles = parsed.root.as_mapping()?.get("tiles")?;
    let out = match (block(&sub, span.value_range.clone()), tiles) {
        (Some(b), Value::Seq(list)) if !list.is_empty() => {
            let pad = " ".repeat(b.indent);
            let mut out = String::with_capacity(sub.len() + 40);
            out.push_str(&sub[..b.end]);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            let Value::Map(m) = &tile else {
                return None;
            };
            let module = m.get("module")?.as_str()?;
            let size = m.get("size")?.as_str()?;
            out.push_str(&format!("{pad}- module: {module}\n{pad}  size: {size}\n"));
            out.push_str(&sub[b.end..]);
            out
        }
        (_, Value::Seq(list)) => {
            let mut list = list.clone();
            list.push(tile);
            set(&sub, "tiles", Value::Seq(list)).ok()?
        }
        (_, Value::Null) => set(&sub, "tiles", Value::Seq(vec![tile])).ok()?,
        _ => return None,
    };
    let mut out = indent_text(&out, inner);
    // Put the `- ` back where the item starts.
    out.replace_range(indent..inner, "- ");
    Some(out)
}

fn set(src: &str, key: &str, value: Value) -> Result<String> {
    let out = edit_yaml(
        src.as_bytes(),
        &[FieldEdit::Set {
            key: key.into(),
            value,
        }],
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    String::from_utf8(out.bytes).context("edited YAML is not UTF-8")
}

/// The lines holding a block collection: from the start of the line where it
/// begins to the end of its last line.
struct Block {
    start: usize,
    end: usize,
    indent: usize,
}

/// The block for a value at `value` in `src`, or `None` when the value is
/// empty, in flow style, or starts on its key's line. `indent` is the column
/// of its keys, or of its `- ` marks for a list.
fn block(src: &str, value: std::ops::Range<usize>) -> Option<Block> {
    if value.is_empty() {
        return None;
    }
    let start = src[..value.start].rfind('\n').map_or(0, |i| i + 1);
    // A block list's value starts after its first `- `.
    let lead = &src[start..value.start];
    let spaces = lead.bytes().take_while(|b| *b == b' ').count();
    if !matches!(&lead[spaces..], "" | "- ") || src[value.start..].starts_with(['{', '[']) {
        return None;
    }
    let end = src[value.end..]
        .find('\n')
        .map_or(src.len(), |i| value.end + i + 1);
    Some(Block {
        start,
        end,
        indent: spaces,
    })
}

/// Byte ranges (within `text`) of each item of a block list whose `- ` marks
/// are at column `indent`.
fn items(text: &str, indent: usize) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = vec![];
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let starts_item = line.len() > indent
            && line.as_bytes()[..indent].iter().all(|b| *b == b' ')
            && (line[indent..].starts_with("- ") || line[indent..].trim_end() == "-");
        if starts_item {
            out.push(at..at + line.len());
        } else if let Some(last) = out.last_mut() {
            last.end = at + line.len();
        }
        at += line.len();
    }
    out
}

fn splice(src: &str, block: &Block, new: &str) -> String {
    let mut out = String::with_capacity(src.len() + new.len());
    out.push_str(&src[..block.start]);
    out.push_str(new);
    out.push_str(&src[block.end..]);
    out
}

/// Remove up to `n` leading spaces from every line.
fn dedent(text: &str, n: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let spaces = line.bytes().take(n).take_while(|b| *b == b' ').count();
        out.push_str(&line[spaces..]);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Add `n` spaces before every line that isn't blank.
fn indent_text(text: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    let mut out = String::with_capacity(text.len() + n * 8);
    for line in text.split_inclusive('\n') {
        if !line.trim().is_empty() {
            out.push_str(&pad);
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(id: &str) -> NewModule {
        NewModule {
            id: id.into(),
            builtin: None,
            package: None,
            demo: false,
            size: None,
            config: serde_json::Value::Null,
        }
    }

    #[test]
    fn adds_inside_modules_and_keeps_comments() {
        let src = "# top\nmodules:\n  # system\n  cpu: {}   # fast\n  disk:\n    refresh: 1m\n\n# screens next\nscreens: []\n";
        let mut m = module("weather");
        m.config = serde_json::json!({"city": "Oslo", "days": 3});
        m.demo = true;
        let out = insert_module(src, &m.id, module_value(&m)).unwrap();
        assert!(
            out.starts_with(
                "# top\nmodules:\n  # system\n  cpu: {}   # fast\n  disk:\n    refresh: 1m\n"
            ),
            "{out}"
        );
        assert!(out.contains("\n# screens next\nscreens: []\n"), "{out}");
        let c = parse(&out).unwrap();
        let w = c.module("weather");
        assert!(w.demo);
        assert_eq!(w.config["city"], "Oslo");
        assert_eq!(w.config["days"], 3);
        assert!(insert_module(&out, "cpu", Value::map()).is_err());
    }

    #[test]
    fn writes_modules_again_when_flow_or_missing() {
        let out = insert_module("theme: dark\nmodules: {}\n", "cpu", Value::map()).unwrap();
        assert!(parse(&out).unwrap().modules.contains_key("cpu"), "{out}");
        assert!(out.starts_with("theme: dark\n"), "{out}");
        let out = insert_module("# only a comment\n", "cpu", Value::map()).unwrap();
        assert!(out.starts_with("# only a comment\n"), "{out}");
        assert!(parse(&out).unwrap().modules.contains_key("cpu"), "{out}");
        let out = insert_module("modules:\ntheme: dark\n", "cpu", Value::map()).unwrap();
        assert!(parse(&out).unwrap().modules.contains_key("cpu"), "{out}");
    }

    #[test]
    fn adds_a_tile_to_the_named_screen_and_keeps_comments() {
        let src = "screens:\n  # first\n  - name: a\n    tiles:\n      - module: cpu   # main\n  - name: b  # second\n    tiles:\n      - module: disk\n        size: wide\n    # end of b\nrotate:\n  interval: 10s\n";
        let out = insert_tile(src, "b", "weather", Size::Small).unwrap();
        assert_eq!(
            out,
            "screens:\n  # first\n  - name: a\n    tiles:\n      - module: cpu   # main\n  - name: b  # second\n    tiles:\n      - module: disk\n        size: wide\n      - module: weather\n        size: small\n    # end of b\nrotate:\n  interval: 10s\n"
        );
        let out = insert_tile(src, "a", "weather", Size::Small).unwrap();
        assert!(
            out.contains("- module: cpu   # main\n      - module: weather\n        size: small\n  - name: b  # second\n"),
            "{out}"
        );
        assert!(insert_tile(src, "nope", "weather", Size::Small).is_err());
    }

    #[test]
    fn adds_a_tile_to_unindented_and_empty_lists() {
        let src =
            "screens:\n- name: a\n  tiles: []   # none yet\n- name: b\n  tiles:\n  - module: cpu\n";
        let out = insert_tile(src, "a", "disk", Size::Medium).unwrap();
        let c = parse(&out).unwrap();
        assert_eq!(c.screens[0].tiles[0].module, "disk");
        assert!(
            out.ends_with("- name: b\n  tiles:\n  - module: cpu\n"),
            "{out}"
        );
        let out = insert_tile(src, "b", "disk", Size::Medium).unwrap();
        assert!(
            out.ends_with("  tiles:\n  - module: cpu\n  - module: disk\n    size: medium\n"),
            "{out}"
        );
        // A flow list of screens is written again.
        let out =
            insert_tile("screens: [{name: a, tiles: []}]\n", "a", "disk", Size::Wide).unwrap();
        assert_eq!(
            parse(&out).unwrap().screens[0].tiles[0].size,
            Some(Size::Wide)
        );
    }

    #[test]
    fn add_module_writes_both_and_rejects_bad_ids() {
        let dir = std::env::temp_dir().join(format!("dashboard-add-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(
            &path,
            "# mine\nmodules:\n  cpu: {}  # keep\nscreens:\n  - name: main\n    tiles:\n      - module: cpu\n",
        )
        .unwrap();
        let mut m = module("prs");
        m.builtin = Some("github-prs".into());
        add_module(&path, &m, Some(("main", Size::Wide))).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with(
                "# mine\nmodules:\n  cpu: {}  # keep\n  prs:\n    builtin: github-prs\n"
            ),
            "{text}"
        );
        let c = parse(&text).unwrap();
        assert_eq!(c.screens[0].tiles[1].module, "prs");
        assert!(add_module(&path, &m, None).is_err());
        assert!(add_module(&path, &module("true"), None).is_err());
        assert!(add_module(&path, &module("a b"), None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adds_inside_a_four_space_modules_block() {
        let src = "modules:\n    cpu: {}    # keep\n    disk:\n        refresh: 5s\n";
        let out = insert_module(src, "prs", module_value(&module("prs"))).unwrap();
        assert!(
            out.starts_with("modules:\n    cpu: {}    # keep\n    disk:\n        refresh: 5s\n"),
            "{out}"
        );
        let c = parse(&out).unwrap();
        assert!(c.modules.contains_key("prs") && c.modules.contains_key("disk"));
    }

    #[test]
    fn add_module_without_a_file_starts_from_the_default_config() {
        let dir =
            std::env::temp_dir().join(format!("dashboard-add-default-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        let default = parse(crate::DEFAULT_CONFIG).unwrap();
        let tile = default
            .screens
            .first()
            .map(|s| (s.name.as_str(), Size::Small));
        add_module(&path, &module("extra"), tile).unwrap();
        let c = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(c.modules.contains_key("extra"));
        // What the default config had is still there.
        assert_eq!(c.screens.len(), default.screens.len());
        for id in default.modules.keys() {
            assert!(c.modules.contains_key(id), "{id}");
        }
        if let Some((name, _)) = tile {
            let s = c.screens.iter().find(|s| s.name == name).unwrap();
            assert_eq!(s.tiles.last().unwrap().module, "extra");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
