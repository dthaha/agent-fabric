-- ADR 004: server-side store schema. Postgres is the op-log (sessions +
-- context_entries) and the SOUL/device registry. The lease authority is
-- Valkey (RESP) — it has no tables here. Timestamps are stored as epoch
-- millis (BIGINT) so they map directly onto pbjson Timestamps without a
-- chrono/time feature dependency.

CREATE TABLE IF NOT EXISTS sessions (
    session_id    TEXT PRIMARY KEY,
    soul_id       TEXT NOT NULL DEFAULT '',
    user_id       TEXT NOT NULL DEFAULT '',
    org_id        TEXT NOT NULL DEFAULT '',
    state         INTEGER NOT NULL DEFAULT 0,
    created_at_ms BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT,
    updated_at_ms BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT
);

CREATE INDEX IF NOT EXISTS idx_sessions_state ON sessions(state);

CREATE TABLE IF NOT EXISTS context_entries (
    entry_id       TEXT PRIMARY KEY,
    session_id     TEXT NOT NULL REFERENCES sessions(session_id),
    seq            BIGINT NOT NULL,
    -- EntryKind name (ENTRY_KIND_*), denormalized for human-friendly querying.
    entry_type     TEXT NOT NULL DEFAULT '',
    kind           INTEGER NOT NULL DEFAULT 0,
    payload        BYTEA NOT NULL,
    lease_holder   TEXT NOT NULL DEFAULT '',
    policy_version TEXT NOT NULL DEFAULT '',
    locus          INTEGER NOT NULL DEFAULT 0,
    created_at_ms  BIGINT NOT NULL,
    received_at_ms BIGINT,
    disposition    TEXT NOT NULL DEFAULT '',
    UNIQUE(session_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_entries_session_seq ON context_entries(session_id, seq);

-- Identity plane (ADR 007): SOUL + device registry on the same Postgres pool.

CREATE TABLE IF NOT EXISTS souls (
    soul_id       TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL,
    org_id        TEXT NOT NULL,
    created_at_ms BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT,
    deleted_at_ms BIGINT,
    UNIQUE(user_id, org_id)
);

CREATE TABLE IF NOT EXISTS devices (
    device_sub      TEXT PRIMARY KEY,
    device_id       TEXT NOT NULL UNIQUE,
    display_name    TEXT NOT NULL DEFAULT '',
    org_id          TEXT NOT NULL DEFAULT '',
    enrolled_at_ms  BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT,
    last_seen_at_ms BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT,
    platform        TEXT NOT NULL DEFAULT 'unknown',
    status          TEXT NOT NULL DEFAULT 'active'
);