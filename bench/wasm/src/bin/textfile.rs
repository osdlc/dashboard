//! The built-in textfile module as a WASI plugin.

#[path = "../../../../crates/modules/src/system/textfile.rs"]
#[allow(dead_code)] // catalog(): the host has its own copy.
mod textfile;

#[allow(unused_imports)]
pub(crate) use sitroom_modules_common::{entry, schema, strings};

/// `crate::common` in the module source.
mod common {
    #[allow(unused_imports)]
    pub(crate) use sitroom_modules_common::*;
}

fn main() {
    bench_wasm_plugins::serve(textfile::TextFile::default());
}
