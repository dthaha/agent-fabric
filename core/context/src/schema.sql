-- Context plane schema. The op-log is append-only: context_entries is keyed
-- by (session_id, seq) and written only by the active lease holder.
-- SQLite runs in WAL mode for concurrent readers during writes.

CREATE TABLE IF NOT EXISTS sessions (
    session_id       TEXT PRIMARY KEY,
    soul_id          TEXT NOT NULL,
    user_id          TEXT NOT NULL,
    state            INTEGER NOT NULL,
    active_lease     TEXT NOT NULL DEFAULT '',
    created_at_ms    INTEGER NOT NULL,
    last_activity_ms INTEGER NOT NULL,
    labels           TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS context_entries (
    session_id     TEXT NOT NULL,
    seq            INTEGER NOT NULL,
    entry_id       TEXT NOT NULL UNIQUE,
    kind           INTEGER NOT NULL,
    payload        BLOB NOT NULL,
    lease_holder   TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    locus          INTEGER NOT NULL,
    created_at_ms  INTEGER NOT NULL,
    PRIMARY KEY (session_id, seq),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id)
);

CREATE INDEX IF NOT EXISTS idx_context_entries_entry_id
    ON context_entries(entry_id);

CREATE TABLE IF NOT EXISTS leases (
    lease_id      TEXT PRIMARY KEY,
    session_id    TEXT NOT NULL,
    holder_id     TEXT NOT NULL,
    locus         INTEGER NOT NULL,
    granted_seq   INTEGER NOT NULL,
    granted_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    state         INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(session_id)
);

CREATE INDEX IF NOT EXISTS idx_leases_session_state
    ON leases(session_id, state);
