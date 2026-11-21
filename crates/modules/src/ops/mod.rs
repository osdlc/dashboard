//! The ops bundle: containers, Kubernetes, the public IP and other
//! infrastructure (Pi-hole, Transmission). Each module sits behind a cargo
//! feature named after its id.

use crate::common::Builtin;
#[allow(unused_imports)]
pub(crate) use crate::common::{entry, schema, strings};

#[cfg(feature = "module-docker")]
pub mod docker;
#[cfg(feature = "module-ipinfo")]
pub mod ipinfo;
#[cfg(feature = "module-kubernetes")]
pub mod kubernetes;
#[cfg(feature = "module-pihole")]
pub mod pihole;
#[cfg(feature = "module-transmission")]
pub mod transmission;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "module-docker")]
    Builtin {
        id: "docker",
        core: false,
        new: || Box::new(docker::Docker::default()),
        catalog: docker::catalog,
    },
    #[cfg(feature = "module-kubernetes")]
    Builtin {
        id: "kubernetes",
        core: false,
        new: || Box::new(kubernetes::Kubernetes::default()),
        catalog: kubernetes::catalog,
    },
    #[cfg(feature = "module-ipinfo")]
    Builtin {
        id: "ipinfo",
        core: false,
        new: || Box::new(ipinfo::IpInfo::default()),
        catalog: ipinfo::catalog,
    },
    #[cfg(feature = "module-pihole")]
    Builtin {
        id: "pihole",
        core: false,
        new: || Box::new(pihole::Pihole::default()),
        catalog: pihole::catalog,
    },
    #[cfg(feature = "module-transmission")]
    Builtin {
        id: "transmission",
        core: false,
        new: || Box::new(transmission::Transmission::default()),
        catalog: transmission::catalog,
    },
];
