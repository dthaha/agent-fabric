-- ADR 008 §3: delegated agent task tracking. One row per K8s Job the
-- orchestrator spawns; the spine (sessions/context_entries, control-plane
-- migrations) remains the record of the work itself — this table tracks the
-- task lifecycle only. State is the lowercase TaskState storage form.

CREATE TABLE IF NOT EXISTS agent_tasks (
    task_id     TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    soul_id     TEXT NOT NULL DEFAULT '',
    state       TEXT NOT NULL,
    job_name    TEXT NOT NULL DEFAULT '',
    lease_id    TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_agent_tasks_session ON agent_tasks(session_id);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_state ON agent_tasks(state);
