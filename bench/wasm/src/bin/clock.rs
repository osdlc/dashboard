//! The built-in clock module as a WASI plugin.

#[path = "../../../../crates/modules/src/fun/clock.rs"]
#[allow(dead_code)] // catalog(): the host has its own copy.
mod clock;

#[allow(unused_imports)]
pub(crate) use sitroom_modules_common::{entry, schema, strings};

/// `crate::common` in the module source.
mod common {
    #[allow(unused_imports)]
    pub(crate) use sitroom_modules_common::*;
}

fn main() {
    bench_wasm_plugins::serve(clock::Clock::default());
}
