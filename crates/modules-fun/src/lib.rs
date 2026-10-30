//! The fun bundle: the demo module, with made-up data in every widget style.
//! Each module sits behind a cargo feature named after its id.

use dashboard_modules_common::Builtin;
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "demo")]
pub mod demo;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "demo")]
    Builtin {
        id: "demo",
        core: false,
        new: || Box::new(demo::Demo::default()),
        catalog: demo::catalog,
    },
];
