//! SOUL + device registry (ADR 007), Postgres-backed per ADR 004. Shares the
//! control-plane's [`PgPool`] (`server/control/src/pg_store.rs`).
//!
//! The SOUL is the one identity Fabric mints: one persistent persona per
//! `(user_id, org_id)`. Users, devices, and orgs come from the customer's
//! IdP/MDM; the `devices` table here is only a *cache* of IdP-enrolled devices
//! that have talked to the server, revocable by admins.

#![cfg(feature = "server-store")]

use sqlx::postgres::{PgPool, PgRow};
use sqlx::{query, Row};

use fabric_context::clock::now_ms;

/// Sighting-write debounce window, in milliseconds: a device whose
/// `last_seen_at` is fresher than this is not written again.
const DEVICE_SIGHTING_DEBOUNCE_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
pub enum SoulError {
    #[error("postgres error: {0}")]
    Postgres(String),
    #[error("soul not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, SoulError>;

fn pg_err(e: sqlx::Error) -> SoulError {
    SoulError::Postgres(e.to_string())
}

/// A Fabric-minted SOUL: one per user per org. `deleted_at_ms` is the GDPR
/// soft-delete marker; a deleted SOUL is never resolved again.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Soul {
    pub soul_id: String,
    pub user_id: String,
    pub org_id: String,
    pub created_at_ms: i64,
    pub deleted_at_ms: Option<i64>,
}

/// A cached record of an IdP/MDM-enrolled device that authenticated to the
/// server. Fabric records sightings; the IdP owns enrollment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Device {
    pub device_sub: String,
    pub device_id: String,
    pub display_name: String,
    pub org_id: String,
    pub enrolled_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub platform: String,
    pub status: String,
}

/// Server-side SOUL + device registry, sharing the control-plane Postgres pool.
#[derive(Clone)]
pub struct SoulRegistry {
    pool: PgPool,
}

impl SoulRegistry {
    /// Wrap an existing pool whose schema has already been migrated (the
    /// `sessions`/`souls`/`devices` tables live in the same init migration).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Resolve the SOUL for `(user_id, org_id)`, creating one with a fresh
    /// UUIDv4 on first sight. Soft-deleted SOULs are skipped: a re-create after
    /// delete mints a new SOUL.
    pub async fn resolve_or_create_soul(&self, user_id: &str, org_id: &str) -> Result<Soul> {
        if let Some(soul) = self.live_soul(user_id, org_id).await? {
            return Ok(soul);
        }
        // The (user_id, org_id) UNIQUE constraint spans deleted rows, so clear
        // the tombstone before re-inserting.
        query("DELETE FROM souls WHERE user_id = $1 AND org_id = $2 AND deleted_at_ms IS NOT NULL")
            .bind(user_id)
            .bind(org_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        let soul_id = uuid::Uuid::new_v4().to_string();
        query("INSERT INTO souls (soul_id, user_id, org_id) VALUES ($1, $2, $3) ON CONFLICT (user_id, org_id) DO NOTHING")
            .bind(&soul_id)
            .bind(user_id)
            .bind(org_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        self.live_soul(user_id, org_id)
            .await?
            .ok_or_else(|| SoulError::NotFound(format!("({user_id}, {org_id})")))
    }

    async fn live_soul(&self, user_id: &str, org_id: &str) -> Result<Option<Soul>> {
        let row = query(
            "SELECT soul_id, user_id, org_id, created_at_ms, deleted_at_ms
             FROM souls WHERE user_id = $1 AND org_id = $2 AND deleted_at_ms IS NULL",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.as_ref().map(Self::soul_row))
    }

    /// Fetch a SOUL by id, including soft-deleted tombstones.
    pub async fn get_soul(&self, soul_id: &str) -> Result<Option<Soul>> {
        let row = query(
            "SELECT soul_id, user_id, org_id, created_at_ms, deleted_at_ms
             FROM souls WHERE soul_id = $1",
        )
        .bind(soul_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.as_ref().map(Self::soul_row))
    }

    /// Soft-delete a SOUL (GDPR right-to-erasure marker). Memory wipe and
    /// session cascade are the memory plane's job; this stamps the tombstone.
    pub async fn delete_soul(&self, soul_id: &str) -> Result<()> {
        query("UPDATE souls SET deleted_at_ms = $1 WHERE soul_id = $2 AND deleted_at_ms IS NULL")
            .bind(now_ms())
            .bind(soul_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    /// Record a device sighting: insert on first authenticated request, update
    /// mutable attributes on later ones. Updates are debounced — a device seen
    /// within the debounce window is not written again (the identity
    /// middleware calls this on EVERY request).
    pub async fn record_device(
        &self,
        device_sub: &str,
        display_name: &str,
        org_id: &str,
        platform: &str,
    ) -> Result<Device> {
        let platform = if platform.is_empty() {
            "unknown"
        } else {
            platform
        };
        let cutoff = now_ms() - DEVICE_SIGHTING_DEBOUNCE_MS;
        query(
            "INSERT INTO devices (device_sub, device_id, display_name, org_id, platform)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (device_sub) DO UPDATE SET
                 last_seen_at_ms = CASE WHEN devices.last_seen_at_ms < $6
                                        THEN EXTRACT(EPOCH FROM now()) * 1000
                                        ELSE devices.last_seen_at_ms END,
                 display_name = CASE WHEN excluded.display_name <> ''
                                     THEN excluded.display_name
                                     ELSE devices.display_name END,
                 org_id = CASE WHEN excluded.org_id <> ''
                               THEN excluded.org_id
                               ELSE devices.org_id END,
                 platform = CASE WHEN excluded.platform <> 'unknown'
                                 THEN excluded.platform
                                 ELSE devices.platform END",
        )
        .bind(device_sub)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(display_name)
        .bind(org_id)
        .bind(platform)
        .bind(cutoff)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        self.get_device(device_sub)
            .await?
            .ok_or_else(|| SoulError::NotFound(device_sub.to_string()))
    }

    /// Fetch a device by its JWT `sub`.
    pub async fn get_device(&self, device_sub: &str) -> Result<Option<Device>> {
        let row = query(
            "SELECT device_sub, device_id, display_name, org_id,
                    enrolled_at_ms, last_seen_at_ms, platform, status
             FROM devices WHERE device_sub = $1",
        )
        .bind(device_sub)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.as_ref().map(Self::device_row))
    }

    /// Revoke a device's access. The IdP still owns enrollment; this only
    /// flips the server-side cache to `revoked`.
    pub async fn revoke_device(&self, device_sub: &str) -> Result<()> {
        query("UPDATE devices SET status = 'revoked' WHERE device_sub = $1")
            .bind(device_sub)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    fn soul_row(row: &PgRow) -> Soul {
        Soul {
            soul_id: row.get("soul_id"),
            user_id: row.get("user_id"),
            org_id: row.get("org_id"),
            created_at_ms: row.get("created_at_ms"),
            deleted_at_ms: row.get("deleted_at_ms"),
        }
    }

    fn device_row(row: &PgRow) -> Device {
        Device {
            device_sub: row.get("device_sub"),
            device_id: row.get("device_id"),
            display_name: row.get("display_name"),
            org_id: row.get("org_id"),
            enrolled_at_ms: row.get("enrolled_at_ms"),
            last_seen_at_ms: row.get("last_seen_at_ms"),
            platform: row.get("platform"),
            status: row.get("status"),
        }
    }
}
