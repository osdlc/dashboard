//! The building blocks (examples/blocks/*.json): one example view per
//! widget type and a few that combine them. Each must be a valid view with
//! nothing for `check_view` to report, so the examples can't drift from the
//! contract.

use std::path::PathBuf;

use dashboard_contract::check::check_view;
use dashboard_contract::{Size, View, Widget};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Block {
    id: String,
    title: String,
    group: String,
    default_size: Size,
    description: String,
    view: View,
}

fn blocks() -> Vec<(String, Block)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/blocks");
    let mut out = vec![];
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        let block: Block =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}.json: {e}"));
        out.push((name, block));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A widget's type name, as JSON spells it.
fn type_name(w: &Widget) -> String {
    serde_json::to_value(w).unwrap()["type"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn every_block_is_a_clean_view() {
    let blocks = blocks();
    assert!(!blocks.is_empty(), "no examples/blocks/*.json");
    for (name, b) in &blocks {
        assert_eq!(&b.id, name, "{name}.json: id must match the file name");
        assert!(
            b.group == "primitives" || b.group == "compositions",
            "{name}: group is {:?}; use primitives or compositions",
            b.group
        );
        assert!(!b.title.trim().is_empty(), "{name}: title is empty");
        assert!(
            !b.description.trim().is_empty(),
            "{name}: description is empty"
        );
        let findings = check_view(&b.view);
        assert!(findings.is_empty(), "{name}: {findings:?}");
    }
}

#[test]
fn every_widget_type_has_a_block() {
    let blocks = blocks();
    let shown: Vec<String> = blocks
        .iter()
        .filter(|(_, b)| b.group == "primitives")
        .map(|(_, b)| type_name(&b.view.root))
        .collect();
    let schema = serde_json::to_value(schemars::schema_for!(Widget)).unwrap();
    for variant in schema["oneOf"].as_array().unwrap() {
        let name = variant["properties"]["type"]["const"].as_str().unwrap();
        assert!(
            shown.iter().any(|s| s == name),
            "no primitive block shows a {name}"
        );
    }
}
