//! Endpoint daemon library crate: the long-running agent service shipped to
//! managed laptops via MDM. Single static binary, no runtime dependencies.
//! Owns the local context store, the offline classifier, seeded models, the
//! tool bridge, and the CUA actuator.
//!
//! The crate is both a binary (the shipped daemon) and a library so the
//! root `tests/` integration crate can drive the daemon's lease client
//! against a real control plane — without the daemon depending on the
//! server at runtime.

pub mod config;
pub mod control_dispatch;
pub mod control_socket;
pub mod http;
pub mod lease;
pub mod state;
