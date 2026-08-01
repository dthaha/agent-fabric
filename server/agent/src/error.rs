//! Typed errors for agent task orchestration. No `anyhow` on this path:
//! every failure mode a caller can act on (lease conflict, missing task,
//! bad request) is a distinct variant.

use fabric_context::StoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentTaskError {
    /// The request failed validation before any lease or Job was touched.
    #[error("invalid task request: {0}")]
    InvalidRequest(String),

    /// No task record exists for the given id.
    #[error("task not found: {0}")]
    TaskNotFound(String),

    /// A [`crate::task_state::TaskRecord`] was asked to make an illegal
    /// transition (e.g. Completed -> Running).
    #[error("invalid task state transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },

    /// A state string from the task-records table didn't parse.
    #[error("unknown task state: {0}")]
    UnknownState(String),

    /// Lease acquisition/release failed (conflict, expiry, backend error).
    #[error(transparent)]
    Lease(#[from] StoreError),

    /// The Kubernetes API call failed.
    #[error(transparent)]
    Kube(#[from] kube::Error),

    /// The task-records table (Postgres) failed.
    #[error(transparent)]
    Store(#[from] sqlx::Error),
}
