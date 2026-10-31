//! The ops bundle: containers, Kubernetes, the public IP and other
//! infrastructure (Pi-hole, Transmission). Each module sits behind a cargo
//! feature named after its id.

use dashboard_modules_common::Builtin;
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "docker")]
pub mod docker;
#[cfg(feature = "ipinfo")]
pub mod ipinfo;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
#[cfg(feature = "pihole")]
pub mod pihole;
#[cfg(feature = "transmission")]
pub mod transmission;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "docker")]
    Builtin {
        id: "docker",
        core: false,
        new: || Box::new(docker::Docker::default()),
        catalog: docker::catalog,
    },
    #[cfg(feature = "kubernetes")]
    Builtin {
        id: "kubernetes",
        core: false,
        new: || Box::new(kubernetes::Kubernetes::default()),
        catalog: kubernetes::catalog,
    },
    #[cfg(feature = "ipinfo")]
    Builtin {
        id: "ipinfo",
        core: false,
        new: || Box::new(ipinfo::IpInfo::default()),
        catalog: ipinfo::catalog,
    },
    #[cfg(feature = "pihole")]
    Builtin {
        id: "pihole",
        core: false,
        new: || Box::new(pihole::Pihole::default()),
        catalog: pihole::catalog,
    },
    #[cfg(feature = "transmission")]
    Builtin {
        id: "transmission",
        core: false,
        new: || Box::new(transmission::Transmission::default()),
        catalog: transmission::catalog,
    },
];
