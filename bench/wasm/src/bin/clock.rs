//! The built-in clock module as a WASI plugin.

#[path = "../../../../crates/modules-fun/src/clock.rs"]
#[allow(dead_code)] // catalog(): the host has its own copy.
mod clock;

#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

fn main() {
    bench_wasm_plugins::serve(clock::Clock::default());
}
