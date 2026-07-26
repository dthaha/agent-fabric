//! Context plane: the spine of Agent Fabric. An append-only, single-writer
//! session op-log with lease-based write enforcement, lease handoff, and
//! deterministic offline reconcile — backed by SQLite (WAL mode) behind the
//! async [`ContextStore`] trait.

pub mod clock;
pub mod conflict;
pub mod constrained_decoder;
pub mod db;
pub mod decoder;
pub mod handoff;
pub mod reconcile;
pub mod store;
pub mod tool_call;

pub use clock::{is_clock_sane, now_ms, MonotonicClock};

pub use conflict::{
    detect_in_region, detect_pair, StructuralConflict, StructuralDisposition, StructuralVerdict,
};
pub use constrained_decoder::{
    verdict_json_schema, ConstrainedDecoder, ConstrainedDecoderConfig,
    SYSTEM_PROMPT as DECODER_SYSTEM_PROMPT,
};
pub use db::{RollbackReport, SqliteContextStore, StoreError};
pub use decoder::{
    build_decoder_input, parse_verdict, ConflictDecoder, ContextTurn, DecoderError, DecoderInput,
    StubDecoder, ToolCallView, OUTPUT_SCHEMA,
};
pub use handoff::{ack_handoff, catch_up, execute_handoff};
pub use reconcile::{reconcile, ReconcileReport, SeqConflict};
pub use store::ContextStore;
