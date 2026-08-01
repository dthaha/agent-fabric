//! Server-side agent loop: runs turns for sessions whose lease is server-side
//! (long-horizon, background, or weak-endpoint cases) and calls endpoint
//! tools over the authenticated bridge. Leased with the context plane.
//!
//! This crate is the control-plane side of ADR 008 §3: it spawns ephemeral
//! K8s Jobs to run pi's `agentLoop` headless for delegated tasks, tracks
//! them in Postgres, and owns the session write lease for the task's
//! lifetime. It never runs the agent loop itself.

pub mod error;
pub mod job_spec;
pub mod orchestrator;
pub mod task_state;

pub use error::AgentTaskError;
pub use orchestrator::{AgentTaskOrchestrator, AgentTaskRequest, ResourceLimits};
pub use task_state::{TaskRecord, TaskState};
