//! Generated protobuf types, shared by every crate in the fabric. Produced
//! by `make proto` (buf generate) into `src/gen/`; this module wires the
//! generated files into the crate.
//!
//! Do not edit by hand. Proto contracts in `proto/` are the source of truth.
#![allow(missing_docs)]
#![allow(clippy::all)]

pub mod context {
    include!("gen/fabric.context.rs");
    include!("gen/fabric/context/fabric.context.serde.rs");
}

pub mod lease {
    include!("gen/fabric.lease.rs");
    include!("gen/fabric/lease/fabric.lease.serde.rs");
}

pub mod policy {
    include!("gen/fabric.policy.rs");
    include!("gen/fabric/policy/fabric.policy.serde.rs");
}

pub mod catalog {
    include!("gen/fabric.catalog.rs");
    include!("gen/fabric/catalog/fabric.catalog.serde.rs");
}

pub mod tools {
    include!("gen/fabric.tools.rs");
    include!("gen/fabric/tools/fabric.tools.serde.rs");
}

pub mod conflict {
    include!("gen/fabric.conflict.rs");
    include!("gen/fabric/conflict/fabric.conflict.serde.rs");
}
