-- Identity plane schema (ADR 007): the SOUL registry and device registry.
-- Fabric is the sole authority on SOULs; devices are a cache of IdP/MDM
-- enrollment ("this device talked to me"), not a directory.

CREATE TABLE IF NOT EXISTS souls (
    soul_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at TEXT,
    UNIQUE(user_id, org_id)
);

CREATE TABLE IF NOT EXISTS devices (
    device_sub TEXT PRIMARY KEY,
    device_id TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL DEFAULT '',
    org_id TEXT NOT NULL DEFAULT '',
    enrolled_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    platform TEXT NOT NULL DEFAULT 'unknown',
    status TEXT NOT NULL DEFAULT 'active'
);
