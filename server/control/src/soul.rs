//! SOUL registry and device registry (ADR 007).
//!
//! The SOUL is the one identity Fabric mints: the agent's persistent
//! persona and memory anchor, one per `(user_id, org_id)` pair. Users,
//! devices, and orgs are consumed from the customer's IdP/MDM — the device
//! table here is only a *cache* of IdP-enrolled devices that have talked to
//! the server, revocable by admins.
//!
//! SQLite is the dev/CI fallback store (same pattern as
//! [`fabric_context::SqliteContextStore`]); Postgres is the production
//! target per ADR 004.

use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

const SCHEMA: &str = include_str!("schema.sql");

/// Sighting-write debounce window, as a SQLite datetime modifier: a device
/// whose `last_seen_at` is younger than this is not written again.
const DEVICE_SIGHTING_DEBOUNCE_SQL: &str = "-5 minutes";

#[derive(Debug, Error)]
pub enum SoulError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, SoulError>;

/// The live (non-soft-deleted) SOUL for `(user_id, org_id)`, if one exists.
fn soul_for_user_org(db: &Connection, user_id: &str, org_id: &str) -> Result<Option<Soul>> {
    Ok(db
        .query_row(
            "SELECT soul_id, user_id, org_id, created_at, deleted_at
             FROM souls
             WHERE user_id = ?1 AND org_id = ?2 AND deleted_at IS NULL",
            params![user_id, org_id],
            |row| {
                Ok(Soul {
                    soul_id: row.get(0)?,
                    user_id: row.get(1)?,
                    org_id: row.get(2)?,
                    created_at: row.get(3)?,
                    deleted_at: row.get(4)?,
                })
            },
        )
        .optional()?)
}

/// A Fabric-minted SOUL: one per user per org. `deleted_at` is the GDPR
/// soft-delete marker; a deleted SOUL is never resolved again.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Soul {
    pub soul_id: String,
    pub user_id: String,
    pub org_id: String,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

/// A cached record of an IdP/MDM-enrolled device that has authenticated to
/// the server. Fabric records sightings; the IdP owns enrollment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Device {
    pub device_sub: String,
    pub device_id: String,
    pub display_name: String,
    pub org_id: String,
    pub enrolled_at: String,
    pub last_seen_at: String,
    pub platform: String,
    pub status: String,
}

/// Server-side SOUL + device registry. Wraps a single SQLite connection
/// behind a mutex so the registry is `Send + Sync` and cheap to clone into
/// handlers and blocking tasks.
#[derive(Clone)]
pub struct SoulRegistry {
    db: Arc<Mutex<Connection>>,
}

impl SoulRegistry {
    /// Open (or create) a registry at `path` and run migrations.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Open an in-memory registry. Used by tests and ephemeral deployments.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Lock the underlying connection. Never hold the guard across a call
    /// to another registry method: every method takes the lock itself.
    fn db(&self) -> MutexGuard<'_, Connection> {
        // A poisoned mutex means a panicking writer mid-statement; the
        // connection itself is still usable. Recover instead of cascading
        // the panic.
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Resolve the SOUL for `(user_id, org_id)`, creating one with a fresh
    /// UUIDv4 on first sight. Soft-deleted SOULs are skipped: if the old
    /// SOUL was deleted, a new one is minted.
    pub fn resolve_or_create_soul(&self, user_id: &str, org_id: &str) -> Result<Soul> {
        let db = self.db();
        if let Some(soul) = soul_for_user_org(&db, user_id, org_id)? {
            return Ok(soul);
        }

        // The (user_id, org_id) UNIQUE constraint spans deleted rows, so a
        // re-create after soft delete must not collide with the tombstone.
        db.execute(
            "DELETE FROM souls WHERE user_id = ?1 AND org_id = ?2 AND deleted_at IS NOT NULL",
            params![user_id, org_id],
        )?;
        let soul_id = uuid::Uuid::new_v4().to_string();
        let insert = db.execute(
            "INSERT INTO souls (soul_id, user_id, org_id) VALUES (?1, ?2, ?3)",
            params![soul_id, user_id, org_id],
        );
        match insert {
            Ok(_) => {
                drop(db);
                self.get_soul(&soul_id)?
                    .ok_or_else(|| SoulError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
            }
            // Lost a create race: another writer inserted the same
            // (user_id, org_id) between our SELECT and INSERT. Re-select
            // and return the winner's row. (Cannot happen under SQLite's
            // single connection; the pattern is for the Postgres target.)
            Err(rusqlite::Error::SqliteFailure(ref e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                soul_for_user_org(&db, user_id, org_id)?
                    .ok_or_else(|| SoulError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
            }
            Err(e) => Err(SoulError::Sqlite(e)),
        }
    }

    /// Fetch a SOUL by id, including soft-deleted ones.
    pub fn get_soul(&self, soul_id: &str) -> Result<Option<Soul>> {
        Ok(self
            .db()
            .query_row(
                "SELECT soul_id, user_id, org_id, created_at, deleted_at
                 FROM souls WHERE soul_id = ?1",
                params![soul_id],
                |row| {
                    Ok(Soul {
                        soul_id: row.get(0)?,
                        user_id: row.get(1)?,
                        org_id: row.get(2)?,
                        created_at: row.get(3)?,
                        deleted_at: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Soft-delete a SOUL (GDPR right-to-erasure marker). Memory wipe and
    /// session cascade are the memory plane's job; this stamps the tombstone.
    pub fn delete_soul(&self, soul_id: &str) -> Result<()> {
        self.db().execute(
            "UPDATE souls SET deleted_at = datetime('now')
             WHERE soul_id = ?1 AND deleted_at IS NULL",
            params![soul_id],
        )?;
        Ok(())
    }

    /// Record a device sighting: insert on first authenticated request,
    /// update `last_seen_at` (and mutable attributes) on subsequent ones.
    /// Updates are debounced: a device seen within the last 5 minutes is
    /// not written again — the identity middleware calls this on EVERY
    /// request, and a per-request UPSERT is pure write amplification.
    pub fn record_device(
        &self,
        device_sub: &str,
        display_name: &str,
        org_id: &str,
        platform: &str,
    ) -> Result<Device> {
        let db = self.db();
        db.execute(
            "INSERT INTO devices (device_sub, device_id, display_name, org_id, platform)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(device_sub) DO UPDATE SET
                 last_seen_at = datetime('now'),
                 display_name = CASE WHEN excluded.display_name != ''
                                     THEN excluded.display_name
                                     ELSE devices.display_name END,
                 org_id = CASE WHEN excluded.org_id != ''
                               THEN excluded.org_id
                               ELSE devices.org_id END,
                 platform = CASE WHEN excluded.platform != 'unknown'
                                 THEN excluded.platform
                                 ELSE devices.platform END
             WHERE devices.last_seen_at < datetime('now', ?6)",
            params![
                device_sub,
                uuid::Uuid::new_v4().to_string(),
                display_name,
                org_id,
                if platform.is_empty() {
                    "unknown"
                } else {
                    platform
                },
                DEVICE_SIGHTING_DEBOUNCE_SQL,
            ],
        )?;
        drop(db);
        self.get_device(device_sub)?
            .ok_or_else(|| SoulError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
    }

    /// Fetch a device by its JWT `sub`.
    pub fn get_device(&self, device_sub: &str) -> Result<Option<Device>> {
        Ok(self
            .db()
            .query_row(
                "SELECT device_sub, device_id, display_name, org_id,
                        enrolled_at, last_seen_at, platform, status
                 FROM devices WHERE device_sub = ?1",
                params![device_sub],
                |row| {
                    Ok(Device {
                        device_sub: row.get(0)?,
                        device_id: row.get(1)?,
                        display_name: row.get(2)?,
                        org_id: row.get(3)?,
                        enrolled_at: row.get(4)?,
                        last_seen_at: row.get(5)?,
                        platform: row.get(6)?,
                        status: row.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// Revoke a device's access. The IdP still owns the enrollment; this
    /// only flips the server-side cache to `revoked`.
    pub fn revoke_device(&self, device_sub: &str) -> Result<()> {
        self.db().execute(
            "UPDATE devices SET status = 'revoked' WHERE device_sub = ?1",
            params![device_sub],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> SoulRegistry {
        SoulRegistry::open_in_memory().unwrap()
    }

    #[test]
    fn resolve_or_create_is_idempotent() {
        let reg = registry();
        let first = reg.resolve_or_create_soul("user-1", "org-1").unwrap();
        assert_eq!(first.user_id, "user-1");
        assert_eq!(first.org_id, "org-1");
        assert!(first.deleted_at.is_none());
        assert!(!first.soul_id.is_empty());

        let second = reg.resolve_or_create_soul("user-1", "org-1").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn souls_are_scoped_per_user_per_org() {
        let reg = registry();
        let a = reg.resolve_or_create_soul("user-1", "org-1").unwrap();
        let b = reg.resolve_or_create_soul("user-1", "org-2").unwrap();
        let c = reg.resolve_or_create_soul("user-2", "org-1").unwrap();
        assert_ne!(a.soul_id, b.soul_id);
        assert_ne!(a.soul_id, c.soul_id);
        assert_ne!(b.soul_id, c.soul_id);
    }

    #[test]
    fn soft_delete_then_recreate_mints_new_soul() {
        let reg = registry();
        let old = reg.resolve_or_create_soul("user-1", "org-1").unwrap();
        reg.delete_soul(&old.soul_id).unwrap();

        let tombstone = reg.get_soul(&old.soul_id).unwrap().unwrap();
        assert!(tombstone.deleted_at.is_some());

        let new = reg.resolve_or_create_soul("user-1", "org-1").unwrap();
        assert_ne!(old.soul_id, new.soul_id);
        assert!(new.deleted_at.is_none());

        // Resolution never returns the deleted SOUL again.
        let again = reg.resolve_or_create_soul("user-1", "org-1").unwrap();
        assert_eq!(new, again);
    }

    #[test]
    fn get_soul_returns_none_for_unknown() {
        let reg = registry();
        assert!(reg.get_soul("nope").unwrap().is_none());
    }

    #[test]
    fn record_device_upserts_and_updates_last_seen() {
        let reg = registry();
        let first = reg
            .record_device("dev-sub-1", "Hermes MacBook", "org-1", "macos")
            .unwrap();
        assert_eq!(first.device_sub, "dev-sub-1");
        assert_eq!(first.display_name, "Hermes MacBook");
        assert_eq!(first.platform, "macos");
        assert_eq!(first.status, "active");
        assert!(!first.device_id.is_empty());

        let second = reg
            .record_device("dev-sub-1", "Hermes MacBook", "org-1", "macos")
            .unwrap();
        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.enrolled_at, second.enrolled_at);
        assert!(second.last_seen_at >= first.last_seen_at);

        // Unknown platform on a later sighting doesn't clobber the real one.
        let third = reg.record_device("dev-sub-1", "", "", "unknown").unwrap();
        assert_eq!(third.display_name, "Hermes MacBook");
        assert_eq!(third.org_id, "org-1");
        assert_eq!(third.platform, "macos");
    }

    #[test]
    fn record_device_sightings_are_debounced() {
        let reg = registry();
        reg.record_device("dev-sub-1", "laptop", "org-1", "macos")
            .unwrap();

        // A sighting within the debounce window is not written: the stored
        // last_seen_at is byte-identical afterwards. Use a sentinel value
        // one minute old so the assertion does not depend on wall-clock
        // resolution.
        reg.db()
            .execute(
                "UPDATE devices SET last_seen_at = datetime('now', '-1 minute')
                 WHERE device_sub = 'dev-sub-1'",
                [],
            )
            .unwrap();
        let fresh = reg.get_device("dev-sub-1").unwrap().unwrap().last_seen_at;
        let again = reg
            .record_device("dev-sub-1", "laptop", "org-1", "macos")
            .unwrap();
        assert_eq!(again.last_seen_at, fresh, "recent sighting must not write");

        // A sighting older than the window IS written.
        reg.db()
            .execute(
                "UPDATE devices SET last_seen_at = datetime('now', '-10 minutes')
                 WHERE device_sub = 'dev-sub-1'",
                [],
            )
            .unwrap();
        let stale = reg.get_device("dev-sub-1").unwrap().unwrap().last_seen_at;
        let updated = reg
            .record_device("dev-sub-1", "laptop", "org-1", "macos")
            .unwrap();
        assert_ne!(updated.last_seen_at, stale, "stale sighting must write");
    }

    #[test]
    fn revoke_device_flips_status() {
        let reg = registry();
        reg.record_device("dev-sub-1", "laptop", "org-1", "linux")
            .unwrap();
        reg.revoke_device("dev-sub-1").unwrap();
        let device = reg.get_device("dev-sub-1").unwrap().unwrap();
        assert_eq!(device.status, "revoked");
    }

    #[test]
    fn get_device_returns_none_for_unknown() {
        let reg = registry();
        assert!(reg.get_device("nope").unwrap().is_none());
    }
}
