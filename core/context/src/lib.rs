//! Context plane: the spine of Agent Fabric. An append-only, single-writer
//! session op-log with lease-based write enforcement, lease handoff, and
//! deterministic offline reconcile — backed by SQLite (WAL mode).

pub mod db;
pub mod handoff;
pub mod reconcile;

pub use db::{ContextStore, StoreError};
pub use handoff::{ack_handoff, catch_up, execute_handoff};
pub use reconcile::{reconcile, ReconcileReport, SeqConflict};
