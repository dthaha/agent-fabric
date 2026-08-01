//! Task lifecycle state machine and the persisted tracking record.
//!
//! ```text
//! Pending ──► Running ──► Completed
//!    │           │
//!    │           ├──────► Failed
//!    │           └──────► Cancelled
//!    ├──────────────────► Failed
//!    └──────────────────► Cancelled
//! ```
//!
//! Terminal states (Completed / Failed / Cancelled) never transition again;
//! `finished_at` is stamped when the record first goes terminal.

use std::fmt;

use chrono::{DateTime, Utc};

use crate::error::AgentTaskError;

/// Lifecycle of a delegated agent task (ADR 008).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// Lease acquired, K8s Job not yet created.
    Pending,
    /// K8s Job active.
    Running,
    /// Job finished successfully.
    Completed,
    /// Job failed, timed out, or vanished before completion was observed.
    Failed,
    /// Lease revoked, user reclaim, or admin kill.
    Cancelled,
}

impl TaskState {
    /// Lowercase wire/storage form (the `state` column in `agent_tasks`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse the storage form back into a state.
    pub fn parse(s: &str) -> Result<Self, AgentTaskError> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(AgentTaskError::UnknownState(other.to_string())),
        }
    }

    /// Terminal states never transition again.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Whether `self -> next` is a legal transition per the state machine.
    pub fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Pending => matches!(next, Self::Running | Self::Failed | Self::Cancelled),
            Self::Running => next.is_terminal(),
            _ => false,
        }
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A delegated agent task as tracked in the `agent_tasks` table.
#[derive(Clone, Debug)]
pub struct TaskRecord {
    pub task_id: String,
    pub session_id: String,
    pub soul_id: String,
    pub state: TaskState,
    /// K8s Job name (`fabric-agent-{session_short}-{uuid_short}`).
    pub job_name: String,
    pub lease_id: String,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl TaskRecord {
    /// Apply a state transition, stamping `finished_at` on the first move
    /// into a terminal state. Illegal transitions are rejected.
    pub fn transition(&mut self, next: TaskState) -> Result<(), AgentTaskError> {
        if !self.state.can_transition_to(next) {
            return Err(AgentTaskError::InvalidTransition {
                from: self.state.as_str().to_string(),
                to: next.as_str().to_string(),
            });
        }
        self.state = next;
        if next.is_terminal() && self.finished_at.is_none() {
            self.finished_at = Some(Utc::now());
        }
        Ok(())
    }

    /// The holder id this task uses on the session's write lease.
    pub fn holder_id(&self) -> String {
        format!("server-agent-{}", self.task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(state: TaskState) -> TaskRecord {
        TaskRecord {
            task_id: "task-1".into(),
            session_id: "sess-1".into(),
            soul_id: "soul-1".into(),
            state,
            job_name: "fabric-agent-sess-1-abcd1234".into(),
            lease_id: "lease-1".into(),
            created_at: Utc::now(),
            finished_at: None,
        }
    }

    #[test]
    fn state_storage_roundtrip() {
        for state in [
            TaskState::Pending,
            TaskState::Running,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ] {
            assert_eq!(TaskState::parse(state.as_str()).unwrap(), state);
        }
        assert!(matches!(
            TaskState::parse("bogus"),
            Err(AgentTaskError::UnknownState(_))
        ));
    }

    #[test]
    fn terminal_states_do_not_transition() {
        for terminal in [
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ] {
            assert!(terminal.is_terminal());
            for next in [
                TaskState::Pending,
                TaskState::Running,
                TaskState::Completed,
                TaskState::Failed,
                TaskState::Cancelled,
            ] {
                assert!(!terminal.can_transition_to(next), "{terminal} -> {next}");
            }
        }
        assert!(!TaskState::Pending.is_terminal());
        assert!(!TaskState::Running.is_terminal());
    }

    #[test]
    fn legal_transitions() {
        assert!(TaskState::Pending.can_transition_to(TaskState::Running));
        assert!(TaskState::Pending.can_transition_to(TaskState::Failed));
        assert!(TaskState::Pending.can_transition_to(TaskState::Cancelled));
        assert!(!TaskState::Pending.can_transition_to(TaskState::Completed));
        assert!(!TaskState::Pending.can_transition_to(TaskState::Pending));

        assert!(TaskState::Running.can_transition_to(TaskState::Completed));
        assert!(TaskState::Running.can_transition_to(TaskState::Failed));
        assert!(TaskState::Running.can_transition_to(TaskState::Cancelled));
        assert!(!TaskState::Running.can_transition_to(TaskState::Pending));
        assert!(!TaskState::Running.can_transition_to(TaskState::Running));
    }

    #[test]
    fn transition_stamps_finished_at_once() {
        let mut r = record(TaskState::Pending);
        assert!(r.finished_at.is_none());
        r.transition(TaskState::Running).unwrap();
        assert!(r.finished_at.is_none());
        r.transition(TaskState::Completed).unwrap();
        let stamped = r
            .finished_at
            .expect("terminal transition stamps finished_at");
        assert_eq!(r.state, TaskState::Completed);

        let err = r.transition(TaskState::Running).unwrap_err();
        assert!(
            matches!(err, AgentTaskError::InvalidTransition { .. }),
            "{err}"
        );
        assert_eq!(r.finished_at, Some(stamped));
    }

    #[test]
    fn invalid_transition_is_an_error() {
        let mut r = record(TaskState::Running);
        let err = r.transition(TaskState::Pending).unwrap_err();
        match err {
            AgentTaskError::InvalidTransition { from, to } => {
                assert_eq!(from, "running");
                assert_eq!(to, "pending");
            }
            other => panic!("expected InvalidTransition, got {other}"),
        }
    }

    #[test]
    fn holder_id_is_derived_from_task_id() {
        assert_eq!(
            record(TaskState::Pending).holder_id(),
            "server-agent-task-1"
        );
    }
}
