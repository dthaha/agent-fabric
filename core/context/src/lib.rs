//! Context plane: the spine of Agent Fabric. An append-only, single-writer
//! session op-log with lease-based write enforcement, lease handoff, and
//! deterministic offline reconcile — backed by SQLite (WAL mode) behind the
//! async [`ContextStore`] trait.

pub mod clock;
pub mod conflict;
#[cfg(feature = "decoder-nemotron")]
pub mod constrained_decoder;
#[cfg(feature = "mediator-nemotron")]
pub mod constrained_mediator;
pub mod db;
pub mod decoder;
pub mod handoff;
pub mod mediator;
pub mod pipeline;
pub mod reconcile;
pub mod store;
pub mod tool_call;

pub use clock::{is_clock_sane, now_ms, MonotonicClock};

pub use conflict::{
    detect_in_region, detect_pair, StructuralConflict, StructuralDisposition, StructuralVerdict,
};
#[cfg(feature = "decoder-nemotron")]
pub use constrained_decoder::{
    verdict_json_schema, ConstrainedDecoder, ConstrainedDecoderConfig,
    SYSTEM_PROMPT as DECODER_SYSTEM_PROMPT,
};
#[cfg(feature = "mediator-nemotron")]
pub use constrained_mediator::{
    proposal_json_schema, ConstrainedMediator, ConstrainedMediatorConfig,
    SYSTEM_PROMPT as MEDIATOR_SYSTEM_PROMPT,
};
pub use db::{RollbackReport, SqliteContextStore, StoreError};
pub use decoder::{
    build_decoder_input, parse_verdict, ConflictDecoder, ContextTurn, DecoderError, DecoderInput,
    StubDecoder, ToolCallView, OUTPUT_SCHEMA,
};
pub use handoff::{ack_handoff, catch_up, execute_handoff};
pub use mediator::{
    parse_proposal, ConflictMediator, MediatorError, MediatorInput, SharedEntityView, StubMediator,
    VerdictView, PROPOSAL_OUTPUT_SCHEMA,
};
pub use pipeline::{ConflictPipeline, PipelineError, DEFAULT_CONTEXT_WINDOW};
pub use reconcile::{reconcile, PolicyViolation, ReconcileReport, SeqConflict};
pub use store::{ContextStore, LeaseAuthority};
