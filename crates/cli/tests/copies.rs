//! Files that crates embed are copies kept inside the crate, so the crates
//! published to crates.io have them. Each must match the file it was copied
//! from; when one fails, copy the original again.

use std::path::Path;

const COPIES: &[(&str, &str)] = &[
    (
        "crates/cli/sdk/sitroom-plugin.ts",
        "sdk/typescript/sitroom-plugin.ts",
    ),
    (
        "crates/cli/sdk/sitroom_plugin.py",
        "sdk/python/sitroom_plugin.py",
    ),
    ("crates/config/demo.yaml", "examples/demo.yaml"),
];

#[test]
fn embedded_copies_match() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (copy, original) in COPIES {
        let read = |path: &str| std::fs::read(root.join(path)).expect(path);
        assert!(
            read(copy) == read(original),
            "{copy} differs from {original}: cp {original} {copy}"
        );
    }
}
