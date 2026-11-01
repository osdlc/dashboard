//! The built-in textfile module as a WASI plugin.

#[path = "../../../../crates/modules-system/src/textfile.rs"]
#[allow(dead_code)] // catalog(): the host has its own copy.
mod textfile;

#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

fn main() {
    bench_wasm_plugins::serve(textfile::TextFile::default());
}
