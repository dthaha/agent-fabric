//! Valkey (RESP) lease authority (ADR 004). The write lease is a hot,
//! coordination primitive — atomic check-and-set, TTL-native expiry — so it
//! lives in an in-memory KV store instead of the op-log's relational database.
//!
//! ## Key layout
//!
//! - `lease:{session_id}` → JSON blob ([`LeaseJson`]).
//! - `leaseid:{lease_id}` → `session_id`: reverse index so
//!   [`LeaseAuthority::lease`] (resolve a lease by id, e.g. for `renew`
//!   tenancy) can find the session the lease belongs to.
//!
//! Both keys share the lease TTL: when it expires both vanish and the lease is
//! gone. There is no `is_expired()` helper and no sweep — Valkey TTL *is* the
//! expiry.
//!
//! ## Atomicity & reply convention
//!
//! Every mutating op is one Lua script (atomic under Valkey's single-threaded
//! execution). Scripts return:
//! - the lease JSON string on success (a value starting with `{`),
//! - `nil` when there is nothing to act on (idempotent `release`),
//! - an error string starting with `!` otherwise. Errors carry detail after a
//!   colon (`!NOTHOLDER:<current holder>`) so the Rust side can map them onto
//!   the shared [`StoreError`] with meaningful context.

use async_trait::async_trait;
use fred::clients::Client as RedisClient;
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use fabric_context::clock::now_ms;
use fabric_context::db::{ms_to_timestamp, Result, StoreError};
use fabric_types::context::Locus;
use fabric_types::lease::{Lease, LeaseState};

const NOTHOLDER_TAG: &str = "!NOTHOLDER:";
const NOLEASE_TAG: &str = "!NOLEASE";
const CONFLICT_TAG: &str = "!CONFLICT";
const CORRUPT_TAG: &str = "!CORRUPT";

/// Authoritative JSON stored at `lease:{session_id}`.
#[derive(Serialize, Deserialize)]
struct LeaseJson {
    lease_id: String,
    holder_id: String,
    locus: i32,
    granted_seq: u64,
    granted_at_ms: i64,
    expires_at_ms: i64,
    granted_by: String,
    preempted_by: String,
}

fn lease_key(session_id: &str) -> String {
    format!("lease:{session_id}")
}
fn leaseid_key(lease_id: &str) -> String {
    format!("leaseid:{lease_id}")
}

// ---- Lua scripts (atomic under Valkey's single-threaded model) ----

/// `acquire`: SET lease:{session} … NX PX ttl; on success also seed the reverse
/// index with the same TTL. Returns the new lease JSON or `!CONFLICT`.
pub const ACQUIRE: &str = r#"
local ok = redis.call('SET', KEYS[1], ARGV[1], 'NX', 'PX', ARGV[2])
if not ok then return '!CONFLICT' end
redis.call('SET', KEYS[2], ARGV[3], 'PX', ARGV[2])
return ARGV[1]
"#;

/// `release`: verify the holder then delete lease + reverse index. `nil` means
/// the key is already gone (idempotent). `!NOTHOLDER:<holder>` on a mismatch.
pub const RELEASE: &str = r#"
local v = redis.call('GET', KEYS[1])
if not v then return nil end
local ok, j = pcall(cjson.decode, v)
if not ok then return '!CORRUPT' end
if j.holder_id ~= ARGV[1] then return '!NOTHOLDER:' .. j.holder_id end
redis.call('DEL', 'leaseid:' .. j.lease_id)
redis.call('DEL', KEYS[1])
return ''
"#;

/// `renew`: verify the holder, refresh both keys' TTL to `ttl`, and rewrite the
/// JSON with the new `expires_at_ms`. Returns the updated JSON.
pub const RENEW: &str = r#"
local v = redis.call('GET', KEYS[1])
if not v then return '!NOLEASE' end
local ok, j = pcall(cjson.decode, v)
if not ok then return '!CORRUPT' end
if j.holder_id ~= ARGV[1] then return '!NOTHOLDER:' .. j.holder_id end
j.expires_at_ms = tonumber(ARGV[2])
local s = cjson.encode(j)
redis.call('SET', KEYS[1], s, 'PX', ARGV[3])
redis.call('PEXPIRE', 'leaseid:' .. j.lease_id, ARGV[3])
return s
"#;

/// `preempt`: force-take the lease. If `new_holder` already holds it, no-op
/// (return the existing lease). Otherwise drop the old reverse index and set
/// the new lease + reverse index.
pub const PREEMPT: &str = r#"
local v = redis.call('GET', KEYS[1])
if v then
  local ok, j = pcall(cjson.decode, v)
  if ok and j.holder_id == ARGV[4] then return v end
  if ok then redis.call('DEL', 'leaseid:' .. j.lease_id) end
end
redis.call('SET', KEYS[1], ARGV[1], 'PX', ARGV[3])
redis.call('SET', KEYS[2], ARGV[2], 'PX', ARGV[3])
return ARGV[1]
"#;

/// `transfer_lease` (H4): atomic release+grant. Verify `from` holds the lease,
/// drop the old reverse index, set the new lease + reverse index.
pub const TRANSFER: &str = r#"
local v = redis.call('GET', KEYS[1])
if not v then return '!NOLEASE' end
local ok, j = pcall(cjson.decode, v)
if not ok then return '!CORRUPT' end
if j.holder_id ~= ARGV[1] then return '!NOTHOLDER:' .. j.holder_id end
redis.call('DEL', 'leaseid:' .. j.lease_id)
redis.call('SET', KEYS[1], ARGV[2], 'PX', ARGV[4])
redis.call('SET', KEYS[2], ARGV[3], 'PX', ARGV[4])
return ARGV[2]
"#;

/// `set_granted_seq`: rewrite `granted_seq`, keeping the TTL (KEEPTTL).
pub const SET_GRANTED_SEQ: &str = r#"
local v = redis.call('GET', KEYS[1])
if not v then return '!NOLEASE' end
local ok, j = pcall(cjson.decode, v)
if not ok then return '!CORRUPT' end
j.granted_seq = tonumber(ARGV[1])
local s = cjson.encode(j)
redis.call('SET', KEYS[1], s, 'KEEPTTL')
return ''
"#;

/// `set_granted_by`: stamp the granting server's identity, keeping the TTL.
pub const SET_GRANTED_BY: &str = r#"
local v = redis.call('GET', KEYS[1])
if not v then return '!NOLEASE' end
local ok, j = pcall(cjson.decode, v)
if not ok then return '!CORRUPT' end
j.granted_by = ARGV[1]
local s = cjson.encode(j)
redis.call('SET', KEYS[1], s, 'KEEPTTL')
return ''
"#;

/// A lease JSON blob starts with `{`; distinguish it from error sentinels.
fn is_json(s: &str) -> bool {
    s.starts_with('{')
}

/// Build a [`Lease`] from its JSON form plus the session the key belongs to.
fn from_json(j: LeaseJson, session_id: &str) -> Lease {
    Lease {
        lease_id: j.lease_id,
        session_id: session_id.to_string(),
        holder_id: j.holder_id,
        locus: j.locus,
        granted_seq: j.granted_seq,
        granted_at: Some(ms_to_timestamp(j.granted_at_ms)),
        expires_at: Some(ms_to_timestamp(j.expires_at_ms)),
        // A key that exists is an ACTIVE lease; released/preempted/expired
        // leases are gone (the TTL fired or the script DEL'd the key).
        state: LeaseState::Active as i32,
        granted_by: j.granted_by,
        preempted_by: j.preempted_by,
    }
}

fn new_lease_json(holder_id: &str, locus: Locus, ttl_ms: i64) -> LeaseJson {
    let now = now_ms();
    LeaseJson {
        lease_id: format!("lease-{}", uuid::Uuid::now_v7()),
        holder_id: holder_id.to_string(),
        locus: locus as i32,
        granted_seq: 0,
        granted_at_ms: now,
        expires_at_ms: now + ttl_ms,
        granted_by: String::new(),
        preempted_by: String::new(),
    }
}

/// Map a `!`-prefixed script error onto the shared [`StoreError`].
fn script_err<T>(s: &str, session_id: &str) -> Result<T> {
    if let Some(holder) = s.strip_prefix(NOTHOLDER_TAG) {
        Err(StoreError::NotLeaseHolder {
            writer: String::new(),
            holder: holder.to_string(),
        })
    } else if s == NOLEASE_TAG {
        Err(StoreError::NoActiveLease(session_id.to_string()))
    } else if s == CONFLICT_TAG {
        Err(StoreError::LeaseConflict(session_id.to_string()))
    } else if s == CORRUPT_TAG {
        Err(StoreError::Valkey("corrupt lease JSON".into()))
    } else {
        Err(StoreError::Valkey(format!("unexpected script reply: {s}")))
    }
}

/// The server-side lease authority backed by any RESP-compatible KV store
/// (Valkey recommended per ADR 004).
#[derive(Clone)]
pub struct ValkeyLeaseAuthority {
    client: RedisClient,
}

impl ValkeyLeaseAuthority {
    /// Connect to `url` (e.g. `redis://127.0.0.1:6379`) and initialize the
    /// client. The connection auto-reconnects after the first `init`.
    pub async fn connect(url: &str) -> Result<Self> {
        let config = Config::from_url(url).map_err(|e| StoreError::Valkey(e.to_string()))?;
        let client = Builder::from_config(config)
            .build()
            .map_err(|e| StoreError::Valkey(e.to_string()))?;
        client
            .init()
            .await
            .map_err(|e| StoreError::Valkey(e.to_string()))?;
        Ok(Self { client })
    }

    async fn get_opt(&self, key: &str) -> Result<Option<String>> {
        self.client
            .get::<Option<String>, _>(key)
            .await
            .map_err(|e| StoreError::Valkey(e.to_string()))
    }

    async fn eval(
        &self,
        script: &str,
        keys: Vec<String>,
        args: Vec<String>,
    ) -> Result<Option<String>> {
        self.client
            .eval::<Option<String>, _, _, _>(script, keys, args)
            .await
            .map_err(|e| StoreError::Valkey(e.to_string()))
    }

    fn parse(blob: &str) -> Result<LeaseJson> {
        serde_json::from_str::<LeaseJson>(blob)
            .map_err(|e| StoreError::Valkey(format!("lease decode error: {e}")))
    }

    /// Read `lease:{session}` and parse, or `None` when there is no active
    /// lease (expired or never granted).
    async fn read_lease(&self, session_id: &str) -> Result<Option<LeaseJson>> {
        match self.get_opt(&lease_key(session_id)).await? {
            None => Ok(None),
            Some(blob) if is_json(&blob) => Ok(Some(Self::parse(&blob)?)),
            Some(blob) => script_err(&blob, session_id),
        }
    }

    // ---- inherent operations used by the control-plane handlers ----

    /// Force-take the lease for `holder` (presence-driven preemption). If the
    /// caller already holds it this is a no-op returning the current lease.
    #[instrument(skip(self), fields(session = %session_id))]
    pub async fn preempt(
        &self,
        session_id: &str,
        holder_id: &str,
        locus: Locus,
        ttl_ms: i64,
    ) -> Result<Lease> {
        if ttl_ms <= 0 {
            return Err(StoreError::InvalidTtl(ttl_ms));
        }
        let json = new_lease_json(holder_id, locus, ttl_ms);
        let lease_id = json.lease_id.clone();
        let blob = serde_json::to_string(&json).map_err(StoreError::Serde)?;
        let out = self
            .eval(
                PREEMPT,
                vec![lease_key(session_id), leaseid_key(&lease_id)],
                vec![
                    blob,
                    session_id.to_string(),
                    ttl_ms.to_string(),
                    holder_id.to_string(),
                ],
            )
            .await?;
        self.decode_or_lease(out, session_id)
    }

    /// Extend an ACTIVE lease's expiry to `now + ttl_ms`. The holder must match.
    pub async fn renew_lease(&self, lease_id: &str, holder_id: &str, ttl_ms: i64) -> Result<Lease> {
        if ttl_ms <= 0 {
            return Err(StoreError::InvalidTtl(ttl_ms));
        }
        let session_id = self
            .get_opt(&leaseid_key(lease_id))
            .await?
            .ok_or_else(|| StoreError::LeaseNotFound(lease_id.to_string()))?;
        let new_expires = now_ms() + ttl_ms;
        let out = self
            .eval(
                RENEW,
                vec![lease_key(&session_id)],
                vec![
                    holder_id.to_string(),
                    new_expires.to_string(),
                    ttl_ms.to_string(),
                ],
            )
            .await?;
        match out.as_deref() {
            Some(s) if is_json(s) => Ok(from_json(Self::parse(s)?, &session_id)),
            Some(s) => Self::map_renew_err(s, holder_id, lease_id),
            None => Err(StoreError::LeaseNotFound(lease_id.to_string())),
        }
    }

    fn map_renew_err(s: &str, writer: &str, lease_id: &str) -> Result<Lease> {
        if let Some(holder) = s.strip_prefix(NOTHOLDER_TAG) {
            Err(StoreError::NotLeaseHolder {
                writer: writer.to_string(),
                holder: holder.to_string(),
            })
        } else if s == NOLEASE_TAG {
            Err(StoreError::LeaseNotFound(lease_id.to_string()))
        } else if s == CORRUPT_TAG {
            Err(StoreError::Valkey("corrupt lease JSON".into()))
        } else {
            Err(StoreError::Valkey(format!("unexpected script reply: {s}")))
        }
    }

    /// Stamp the granting server's identity into `granted_by` (audit). TTL is
    /// preserved.
    pub async fn set_granted_by(&self, lease_id: &str, granted_by: &str) -> Result<()> {
        let session_id = self
            .get_opt(&leaseid_key(lease_id))
            .await?
            .ok_or_else(|| StoreError::LeaseNotFound(lease_id.to_string()))?;
        let out = self
            .eval(
                SET_GRANTED_BY,
                vec![lease_key(&session_id)],
                vec![granted_by.to_string()],
            )
            .await?;
        match out.as_deref() {
            None | Some("") => Ok(()),
            Some(s) if s == NOLEASE_TAG => Err(StoreError::LeaseNotFound(lease_id.to_string())),
            Some(s) if s == CORRUPT_TAG => Err(StoreError::Valkey("corrupt lease JSON".into())),
            Some(s) => Err(StoreError::Valkey(format!("unexpected script reply: {s}"))),
        }
    }

    /// Reduce a script reply to either a parsed [`Lease`] or an error. `None`
    /// means the lease vanished mid-op.
    fn decode_or_lease(&self, out: Option<String>, session_id: &str) -> Result<Lease> {
        match out {
            None => Err(StoreError::NoActiveLease(session_id.to_string())),
            Some(s) if s.is_empty() => Err(StoreError::NoActiveLease(session_id.to_string())),
            Some(s) if is_json(&s) => Ok(from_json(Self::parse(&s)?, session_id)),
            Some(s) => script_err(&s, session_id),
        }
    }
}

#[async_trait]
impl fabric_context::store::LeaseAuthority for ValkeyLeaseAuthority {
    #[instrument(skip(self), fields(session = %session_id))]
    async fn acquire_lease(
        &self,
        session_id: &str,
        holder_id: &str,
        locus: Locus,
        ttl_ms: i64,
    ) -> Result<Lease> {
        if ttl_ms <= 0 {
            return Err(StoreError::InvalidTtl(ttl_ms));
        }
        let json = new_lease_json(holder_id, locus, ttl_ms);
        let lease_id = json.lease_id.clone();
        let blob = serde_json::to_string(&json).map_err(StoreError::Serde)?;
        let out = self
            .eval(
                ACQUIRE,
                vec![lease_key(session_id), leaseid_key(&lease_id)],
                vec![blob, ttl_ms.to_string(), session_id.to_string()],
            )
            .await?;
        match out.as_deref() {
            Some(s) if s == CONFLICT_TAG => Err(StoreError::LeaseConflict(session_id.to_string())),
            Some(s) if is_json(s) => Ok(from_json(Self::parse(s)?, session_id)),
            Some(s) => script_err(s, session_id),
            None => Err(StoreError::LeaseConflict(session_id.to_string())),
        }
    }

    async fn release_lease(&self, session_id: &str, holder_id: &str) -> Result<()> {
        let out = self
            .eval(
                RELEASE,
                vec![lease_key(session_id)],
                vec![holder_id.to_string()],
            )
            .await?;
        match out.as_deref() {
            // nil = already released (idempotent); "" = released now.
            None | Some("") => Ok(()),
            Some(s) if let Some(holder) = s.strip_prefix(NOTHOLDER_TAG) => {
                Err(StoreError::NotLeaseHolder {
                    writer: holder_id.to_string(),
                    holder: holder.to_string(),
                })
            }
            Some(s) if s == CORRUPT_TAG => Err(StoreError::Valkey("corrupt lease JSON".into())),
            Some(s) => script_err(s, session_id),
        }
    }

    async fn lease(&self, lease_id: &str) -> Result<Lease> {
        let session_id = self
            .get_opt(&leaseid_key(lease_id))
            .await?
            .ok_or_else(|| StoreError::LeaseNotFound(lease_id.to_string()))?;
        match self.read_lease(&session_id).await? {
            Some(j) => Ok(from_json(j, &session_id)),
            None => Err(StoreError::LeaseNotFound(lease_id.to_string())),
        }
    }

    async fn active_lease(&self, session_id: &str) -> Result<Option<Lease>> {
        Ok(self
            .read_lease(session_id)
            .await?
            .map(|j| from_json(j, session_id)))
    }

    async fn verify_writer(&self, session_id: &str, writer: &str) -> Result<Lease> {
        match self.read_lease(session_id).await? {
            None => Err(StoreError::NoActiveLease(session_id.to_string())),
            Some(j) if j.holder_id != writer => Err(StoreError::NotLeaseHolder {
                writer: writer.to_string(),
                holder: j.holder_id,
            }),
            Some(j) => Ok(from_json(j, session_id)),
        }
    }

    async fn set_granted_seq(&self, lease_id: &str, granted_seq: u64) -> Result<()> {
        let session_id = self
            .get_opt(&leaseid_key(lease_id))
            .await?
            .ok_or_else(|| StoreError::LeaseNotFound(lease_id.to_string()))?;
        let out = self
            .eval(
                SET_GRANTED_SEQ,
                vec![lease_key(&session_id)],
                vec![granted_seq.to_string()],
            )
            .await?;
        match out.as_deref() {
            None | Some("") => Ok(()),
            Some(s) if s == NOLEASE_TAG => Err(StoreError::LeaseNotFound(lease_id.to_string())),
            Some(s) if s == CORRUPT_TAG => Err(StoreError::Valkey("corrupt lease JSON".into())),
            Some(s) => script_err(s, &session_id),
        }
    }

    async fn transfer_lease(
        &self,
        session_id: &str,
        from_holder: &str,
        to_holder: &str,
        locus: Locus,
        ttl_ms: i64,
        freeze_seq: u64,
    ) -> Result<Lease> {
        if ttl_ms <= 0 {
            return Err(StoreError::InvalidTtl(ttl_ms));
        }
        let mut json = new_lease_json(to_holder, locus, ttl_ms);
        json.granted_seq = freeze_seq;
        let new_lease_id = json.lease_id.clone();
        let blob = serde_json::to_string(&json).map_err(StoreError::Serde)?;
        let out = self
            .eval(
                TRANSFER,
                vec![lease_key(session_id), leaseid_key(&new_lease_id)],
                vec![
                    from_holder.to_string(),
                    blob,
                    session_id.to_string(),
                    ttl_ms.to_string(),
                ],
            )
            .await?;
        match out.as_deref() {
            Some(s) if is_json(s) => Ok(from_json(Self::parse(s)?, session_id)),
            Some(s) if let Some(holder) = s.strip_prefix(NOTHOLDER_TAG) => {
                Err(StoreError::NotLeaseHolder {
                    writer: from_holder.to_string(),
                    holder: holder.to_string(),
                })
            }
            Some(s) if s == NOLEASE_TAG => Err(StoreError::NoActiveLease(session_id.to_string())),
            None => Err(StoreError::NoActiveLease(session_id.to_string())),
            Some(s) if s == CORRUPT_TAG => Err(StoreError::Valkey("corrupt lease JSON".into())),
            Some(s) => script_err(s, session_id),
        }
    }
}
