//! Context plane: the spine of Agent Fabric. An append-only, single-writer
//! session op-log with lease-based write enforcement, lease handoff, and
//! deterministic offline reconcile — backed by SQLite (WAL mode) behind the
//! async [`ContextStore`] trait.

pub mod clock;
pub mod conflict;
pub mod db;
pub mod handoff;
pub mod reconcile;
pub mod store;
pub mod tool_call;

pub use clock::{is_clock_sane, now_ms, MonotonicClock};

pub use conflict::{
    detect_in_region, detect_pair, StructuralConflict, StructuralDisposition, StructuralVerdict,
};
pub use db::{RollbackReport, SqliteContextStore, StoreError};
pub use handoff::{ack_handoff, catch_up, execute_handoff};
pub use reconcile::{reconcile, ReconcileReport, SeqConflict};
pub use store::ContextStore;
