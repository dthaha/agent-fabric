//! Control-socket method dispatch: routes NDJSON requests from agent
//! harnesses (e.g. `@fabric/pi-session-backend`, ADR 008) into the local
//! SQLite context store. The daemon stores entry payloads OPAQUELY — it
//! never parses pi's entry types beyond the `id`/`parentId` fields needed
//! for fork rewriting and path walking.
//!
//! Write path invariants (non-negotiable, AGENTS.md): `entry.append` is
//! lease-gated by the store itself (single writer) and passes through the
//! policy gate (kill switch + DLP scan) before anything is persisted.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use tracing::warn;

use fabric_context::{now_ms, StoreError, DEFAULT_LEASE_TTL_MS};
use fabric_types::context::{ContextEntry, EntryKind, Locus, SessionMeta, SessionState};
use fabric_types::policy::DlpAction;

use crate::state::DaemonState;

/// Errors returned to the socket client. Codes match the SessionError
/// codes of `@fabric/pi-session-backend`.
#[derive(Debug)]
pub enum ControlError {
    NotFound(String),
    InvalidSession(String),
    InvalidEntry(String),
    InvalidForkTarget(String),
    Storage(String),
    Unknown(String),
}

impl ControlError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::InvalidSession(_) => "invalid_session",
            Self::InvalidEntry(_) => "invalid_entry",
            Self::InvalidForkTarget(_) => "invalid_fork_target",
            Self::Storage(_) => "storage",
            Self::Unknown(_) => "unknown",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(m)
            | Self::InvalidSession(m)
            | Self::InvalidEntry(m)
            | Self::InvalidForkTarget(m)
            | Self::Storage(m)
            | Self::Unknown(m) => m,
        }
    }
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ControlError {}

/// Map store failures onto the wire error codes. Lease/state failures are
/// session-scoped client errors; everything else is a storage fault.
fn map_store_err(e: StoreError) -> ControlError {
    match e {
        StoreError::SessionNotFound(_) => ControlError::NotFound(e.to_string()),
        StoreError::NoActiveLease(_)
        | StoreError::NotLeaseHolder { .. }
        | StoreError::SessionNotActive { .. }
        | StoreError::LeaseConflict(_)
        | StoreError::LeaseNotActive(_)
        | StoreError::LeaseExpired(_) => ControlError::InvalidSession(e.to_string()),
        other => ControlError::Storage(other.to_string()),
    }
}

/// Route a control-socket method to the context store. Blocking (SQLite);
/// the socket layer runs this on the blocking thread pool.
pub fn dispatch(state: &DaemonState, method: &str, params: &Value) -> Result<Value, ControlError> {
    match method {
        "session.create" => session_create(state, params),
        "session.load" => session_load(state, params),
        "session.list" => session_list(state),
        "session.delete" => session_delete(state, params),
        "session.fork" => session_fork(state, params),
        "session.head" => session_head(state, params),
        "entry.append" => entry_append(state, params),
        "entry.read" => entry_read(state, params),
        "entry.list" => entry_list(state, params),
        "entry.path" => entry_path(state, params),
        other => Err(ControlError::Unknown(format!("unknown method: {other}"))),
    }
}

/// The lease holder for a request: explicit `holder_id` param, else the
/// daemon's own device id (the pi backend does not send one — the daemon
/// is the local writer on behalf of the harness).
fn holder_id(state: &DaemonState, params: &Value) -> String {
    params
        .get("holder_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&state.cfg.device_id)
        .to_string()
}

fn required_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, ControlError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ControlError::InvalidSession(format!("missing required param: {key}")))
}

/// Ensure `holder` holds the session's write lease: no-op when it already
/// does, acquire when the session has no active writer, reject when
/// another holder owns it.
fn ensure_lease(
    store: &fabric_context::SqliteContextStore,
    session_id: &str,
    holder: &str,
) -> Result<(), ControlError> {
    match store.active_lease(session_id).map_err(map_store_err)? {
        Some(lease) if lease.holder_id == holder => Ok(()),
        Some(lease) => Err(ControlError::InvalidSession(format!(
            "write lease for session {session_id} is held by '{}'",
            lease.holder_id
        ))),
        None => store
            .acquire_lease(session_id, holder, Locus::Endpoint, DEFAULT_LEASE_TTL_MS)
            .map(|_| ())
            .map_err(map_store_err),
    }
}

fn session_meta_json(
    store: &fabric_context::SqliteContextStore,
    meta: &SessionMeta,
) -> Result<Value, ControlError> {
    let created_ms = meta
        .created_at
        .as_ref()
        .map(|t| {
            t.seconds
                .saturating_mul(1000)
                .saturating_add(i64::from(t.nanos) / 1_000_000)
        })
        .unwrap_or(0);
    let head_seq = store.head_seq(&meta.session_id).unwrap_or(0);
    Ok(json!({
        "id": meta.session_id,
        "created_at": rfc3339_from_ms(created_ms),
        "state": SessionState::try_from(meta.state)
            .map(|s| s.as_str_name())
            .unwrap_or("UNKNOWN"),
        "head_seq": head_seq,
    }))
}

fn session_create(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let now = now_ms();
    let meta = SessionMeta {
        session_id: id.clone(),
        soul_id: String::new(),
        user_id: state.cfg.user_id.clone(),
        state: SessionState::Active as i32,
        active_lease: String::new(),
        created_at: Some(fabric_context::db::ms_to_timestamp(now)),
        last_activity: Some(fabric_context::db::ms_to_timestamp(now)),
        labels: HashMap::new(),
        org_id: state.cfg.org_id.clone(),
    };
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    store.create_session(&meta).map_err(map_store_err)?;
    // create_session is idempotent; a pre-existing id may point at a
    // deleted (archived) session, which must not come back to life.
    let meta = store.session(&id).map_err(map_store_err)?;
    if meta.state != SessionState::Active as i32 {
        return Err(ControlError::InvalidSession(format!(
            "session {id} is not active"
        )));
    }
    ensure_lease(&store, &id, &holder_id(state, params))?;
    session_meta_json(&store, &meta)
}

fn session_load(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let id = required_str(params, "id")?;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let meta = store.session(id).map_err(map_store_err)?;
    if meta.state != SessionState::Active as i32 {
        return Err(ControlError::InvalidSession(format!(
            "session {id} is not active"
        )));
    }
    ensure_lease(&store, id, &holder_id(state, params))?;
    session_meta_json(&store, &meta)
}

fn session_list(state: &DaemonState) -> Result<Value, ControlError> {
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let sessions = store.list_active_sessions().map_err(map_store_err)?;
    let mut out = Vec::with_capacity(sessions.len());
    for meta in &sessions {
        out.push(session_meta_json(&store, meta)?);
    }
    Ok(json!({ "sessions": out }))
}

/// Delete = terminal lifecycle transition. The op-log itself is
/// append-only; deleting archives the session so it vanishes from listings
/// and rejects further appends.
fn session_delete(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let id = required_str(params, "id")?;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let meta = store.session(id).map_err(map_store_err)?;
    if store.active_lease(id).map_err(map_store_err)?.is_some() {
        store
            .revoke_lease(id, "session.delete via control socket")
            .map_err(map_store_err)?;
    }
    match SessionState::try_from(meta.state) {
        Ok(SessionState::Active) => {
            store.complete(id).map_err(map_store_err)?;
            store.archive(id).map_err(map_store_err)?;
        }
        Ok(SessionState::Suspended) => {
            store.resume(id).map_err(map_store_err)?;
            store.complete(id).map_err(map_store_err)?;
            store.archive(id).map_err(map_store_err)?;
        }
        Ok(SessionState::Completed) => store.archive(id).map_err(map_store_err)?,
        Ok(SessionState::Archived) => {}
        other => {
            return Err(ControlError::InvalidSession(format!(
                "cannot delete session {id} in state {}",
                other.map(|s| s.as_str_name()).unwrap_or("UNKNOWN")
            )))
        }
    }
    Ok(json!({ "id": id, "deleted": true }))
}

fn session_fork(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let source_id = required_str(params, "source_id")?;
    let selection = params
        .get("selection")
        .ok_or_else(|| ControlError::InvalidForkTarget("missing selection".into()))?;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let source = store.session(source_id).map_err(map_store_err)?;

    let entries = store.entries_since(source_id, 0).map_err(map_store_err)?;
    let kind = selection.get("kind").and_then(Value::as_str);
    let cutoff = match kind {
        Some("all") => entries.last().map(|e| e.seq).unwrap_or(0),
        Some("before_user_message") | Some("through_entry") => {
            let target_id = selection
                .get("entryId")
                .or_else(|| selection.get("entry_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ControlError::InvalidForkTarget("selection missing entry id".into())
                })?;
            let target = entries
                .iter()
                .find(|e| e.entry_id == target_id)
                .ok_or_else(|| {
                    ControlError::InvalidForkTarget(format!(
                        "entry {target_id} not found in session {source_id}"
                    ))
                })?;
            if kind == Some("before_user_message") {
                target.seq.saturating_sub(1)
            } else {
                target.seq
            }
        }
        other => {
            return Err(ControlError::InvalidForkTarget(format!(
                "unknown selection kind: {}",
                other.unwrap_or("<missing>")
            )))
        }
    };
    let selected: Vec<&ContextEntry> = entries.iter().filter(|e| e.seq <= cutoff).collect();

    let new_id = params
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let now = now_ms();
    let meta = SessionMeta {
        session_id: new_id.clone(),
        soul_id: source.soul_id.clone(),
        user_id: source.user_id.clone(),
        state: SessionState::Active as i32,
        active_lease: String::new(),
        created_at: Some(fabric_context::db::ms_to_timestamp(now)),
        last_activity: Some(fabric_context::db::ms_to_timestamp(now)),
        labels: source.labels.clone(),
        org_id: source.org_id.clone(),
    };
    store.create_session(&meta).map_err(map_store_err)?;

    let holder = holder_id(state, params);
    ensure_lease(&store, &new_id, &holder)?;

    // Forked entries get fresh ids (entry_id is globally unique in the
    // store); `id`/`parentId` inside the opaque payload are rewritten
    // through the old→new map so the fork's parent chain stays navigable.
    let id_map: HashMap<&str, String> = selected
        .iter()
        .map(|e| (e.entry_id.as_str(), uuid::Uuid::now_v7().to_string()))
        .collect();
    for source_entry in selected {
        let mut payload: Value = serde_json::from_slice(&source_entry.payload)
            .map_err(|e| ControlError::Storage(format!("decoding entry payload: {e}")))?;
        let new_entry_id = id_map[source_entry.entry_id.as_str()].clone();
        payload["id"] = Value::String(new_entry_id.clone());
        for key in ["parentId", "parent_id"] {
            if let Some(parent) = payload.get(key).and_then(Value::as_str) {
                let mapped = id_map
                    .get(parent)
                    .cloned()
                    .unwrap_or_else(|| parent.to_string());
                payload[key] = Value::String(mapped);
            }
        }
        let mut entry = ContextEntry {
            entry_id: new_entry_id,
            session_id: new_id.clone(),
            seq: 0,
            kind: source_entry.kind,
            payload: serde_json::to_vec(&payload)
                .map_err(|e| ControlError::Storage(format!("encoding entry payload: {e}")))?,
            lease_holder: holder.clone(),
            policy_version: source_entry.policy_version.clone(),
            locus: source_entry.locus,
            created_at: None,
            received_at: None,
            disposition: source_entry.disposition.clone(),
        };
        store.append_entry(&mut entry).map_err(map_store_err)?;
    }
    let meta = store.session(&new_id).map_err(map_store_err)?;
    session_meta_json(&store, &meta)
}

fn session_head(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let session_id = required_str(params, "session_id")?;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    store.session(session_id).map_err(map_store_err)?;
    let head = store.head_seq(session_id).map_err(map_store_err)?;
    if head == 0 {
        return Ok(json!({ "leaf_id": Value::Null }));
    }
    let leaf = store
        .entry_at_seq(session_id, head)
        .map_err(map_store_err)?
        .map(|e| e.entry_id);
    Ok(json!({ "leaf_id": leaf }))
}

fn entry_append(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let session_id = required_str(params, "session_id")?;
    let entry_value = params
        .get("entry")
        .filter(|v| v.is_object())
        .ok_or_else(|| ControlError::InvalidEntry("missing or invalid entry".into()))?;
    let entry_id = entry_value
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ControlError::InvalidEntry("entry missing id".into()))?
        .to_string();
    let entry_type = entry_value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("");

    // POLICY GATE (non-negotiable): kill switch denies all appends; the
    // DLP scan runs over the serialized payload before anything persists.
    let (killed, dlp, policy_version) = {
        let policy = state.policy.read().unwrap_or_else(|e| e.into_inner());
        let gate = policy.gate();
        let version = policy.endpoint_version().unwrap_or("").to_string();
        let killed = gate.is_killed();
        let dlp = if killed {
            None
        } else {
            Some(
                gate.scan_dlp(&entry_value.to_string())
                    .map_err(|e| ControlError::Storage(format!("DLP scan failed: {e}")))?,
            )
        };
        (killed, dlp, version)
    };
    if killed {
        return Err(ControlError::InvalidEntry(
            "policy kill switch is active: appends denied".into(),
        ));
    }
    let mut payload_value = entry_value.clone();
    if let Some(dlp) = dlp {
        match dlp.action {
            Some(DlpAction::Block) => {
                return Err(ControlError::InvalidEntry(format!(
                    "entry blocked by DLP policy ({})",
                    dlp.matched_patterns.join(", ")
                )))
            }
            Some(DlpAction::Redact) => {
                payload_value = serde_json::from_str(&dlp.redacted_content).map_err(|_| {
                    ControlError::InvalidEntry(
                        "entry blocked by DLP policy (redaction would corrupt payload)".into(),
                    )
                })?;
            }
            _ => {}
        }
    }

    let holder = holder_id(state, params);
    let mut entry = ContextEntry {
        entry_id: entry_id.clone(),
        session_id: session_id.to_string(),
        seq: 0,
        kind: kind_from_entry_type(entry_type),
        payload: serde_json::to_vec(&payload_value)
            .map_err(|e| ControlError::InvalidEntry(format!("encoding entry payload: {e}")))?,
        lease_holder: holder,
        policy_version,
        locus: Locus::Endpoint as i32,
        created_at: None,
        received_at: None,
        disposition: String::new(),
    };
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    // The store enforces session-ACTIVE and lease ownership transactionally.
    let seq = store.append_entry(&mut entry).map_err(map_store_err)?;
    Ok(json!({ "id": entry_id, "seq": seq }))
}

fn entry_read(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let session_id = required_str(params, "session_id")?;
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ControlError::InvalidEntry("missing required param: id".into()))?;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    let entry = store.entry_by_id(id).map_err(map_store_err)?;
    match entry {
        Some(e) if e.session_id == session_id => {
            let payload = decode_payload(&e)?;
            Ok(json!({ "seq": e.seq, "entry": payload }))
        }
        // Missing entries read back as null, matching the SessionReader
        // contract (`readEntry` returns undefined).
        _ => Ok(json!({ "seq": Value::Null, "entry": Value::Null })),
    }
}

fn entry_list(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let session_id = required_str(params, "session_id")?;
    let after_seq = params.get("after_seq").and_then(Value::as_u64).unwrap_or(0);
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    store.session(session_id).map_err(map_store_err)?;
    let entries = store
        .entries_since(session_id, after_seq)
        .map_err(map_store_err)?;
    let mut out = Vec::with_capacity(entries.len().min(limit));
    for entry in entries.iter().take(limit) {
        out.push(decode_payload(entry)?);
    }
    Ok(json!({ "entries": out }))
}

/// Walk the `parentId` chain from the leaf toward the root (or a
/// compaction boundary — any entry whose parent is absent from the log).
/// Returns entries leaf-first, matching pi's readPathToRootOrCompaction.
fn entry_path(state: &DaemonState, params: &Value) -> Result<Value, ControlError> {
    let session_id = required_str(params, "session_id")?;
    let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
    store.session(session_id).map_err(map_store_err)?;
    let Some(leaf_id) = params.get("leaf_id").and_then(Value::as_str) else {
        return Ok(json!({ "entries": [] }));
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(leaf_id.to_string());
    while let Some(id) = current {
        // Cycle guard: a malformed chain must not spin the daemon.
        if !seen.insert(id.clone()) {
            warn!(session = session_id, entry = %id, "parent cycle detected; truncating path");
            break;
        }
        let Some(entry) = store.entry_by_id(&id).map_err(map_store_err)? else {
            break;
        };
        if entry.session_id != session_id {
            break;
        }
        let payload = decode_payload(&entry)?;
        current = payload
            .get("parentId")
            .or_else(|| payload.get("parent_id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        out.push(payload);
    }
    if out.is_empty() {
        return Err(ControlError::NotFound(format!(
            "entry {leaf_id} not found in session {session_id}"
        )));
    }
    Ok(json!({ "entries": out }))
}

fn decode_payload(entry: &ContextEntry) -> Result<Value, ControlError> {
    serde_json::from_slice(&entry.payload).map_err(|e| {
        ControlError::Storage(format!("decoding entry {} payload: {e}", entry.entry_id))
    })
}

/// Coarse kind mapping for metadata/indexing only. The payload stays
/// authoritative; unknown harness types map to UNSPECIFIED.
fn kind_from_entry_type(t: &str) -> i32 {
    match t {
        "user_message" => EntryKind::UserMessage as i32,
        "assistant_message" => EntryKind::AssistantMessage as i32,
        "tool_call" => EntryKind::ToolCall as i32,
        "tool_result" => EntryKind::ToolResult as i32,
        "system_event" | "compaction" | "branch_summary" => EntryKind::SystemEvent as i32,
        "goal_update" => EntryKind::GoalUpdate as i32,
        "plan_step" => EntryKind::PlanStep as i32,
        "handoff_marker" => EntryKind::HandoffMarker as i32,
        "deferred_intent" => EntryKind::DeferredIntent as i32,
        _ => EntryKind::Unspecified as i32,
    }
}

/// Format epoch milliseconds as RFC 3339 (UTC). Self-contained: the
/// endpoint binary keeps its dependency surface minimal.
fn rfc3339_from_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        day_secs / 3600,
        (day_secs % 3600) / 60,
        day_secs % 60,
    )
}

/// Days since Unix epoch → (year, month, day). Howard Hinnant's
/// civil-from-days algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::policy::{DlpPattern, EndpointPolicy};

    use crate::config::DaemonConfig;

    fn test_state() -> std::sync::Arc<DaemonState> {
        let store = fabric_context::SqliteContextStore::open_in_memory().unwrap();
        DaemonState::new(DaemonConfig::default(), store)
    }

    fn entry(id: &str, parent: Option<&str>) -> Value {
        json!({
            "type": "user_message",
            "id": id,
            "parentId": parent,
            "timestamp": "2026-07-31T00:00:00.000Z",
            "text": format!("message {id}"),
        })
    }

    fn create_session(state: &DaemonState) -> String {
        let result = dispatch(state, "session.create", &json!({})).unwrap();
        result["id"].as_str().unwrap().to_string()
    }

    fn append(state: &DaemonState, session_id: &str, entry: Value) -> Result<Value, ControlError> {
        dispatch(
            state,
            "entry.append",
            &json!({ "session_id": session_id, "entry": entry }),
        )
    }

    #[test]
    fn create_list_load_delete_lifecycle() {
        let state = test_state();
        let id = create_session(&state);

        let list = dispatch(&state, "session.list", &json!({})).unwrap();
        let sessions = list["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["id"], id);
        assert!(sessions[0]["created_at"].as_str().unwrap().ends_with('Z'));

        let loaded = dispatch(&state, "session.load", &json!({ "id": id })).unwrap();
        assert_eq!(loaded["id"], id);
        assert_eq!(loaded["state"], "SESSION_STATE_ACTIVE");

        dispatch(&state, "session.delete", &json!({ "id": id })).unwrap();
        let list = dispatch(&state, "session.list", &json!({})).unwrap();
        assert_eq!(list["sessions"].as_array().unwrap().len(), 0);

        let err = dispatch(&state, "session.load", &json!({ "id": id })).unwrap_err();
        assert_eq!(err.code(), "invalid_session");
    }

    #[test]
    fn load_missing_session_is_not_found() {
        let state = test_state();
        let err = dispatch(&state, "session.load", &json!({ "id": "nope" })).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn append_is_lease_gated() {
        let state = test_state();
        // A session created out-of-band (no lease held by anyone) rejects
        // appends until a holder acquires the lease via create/load.
        {
            let store = state.store.lock().unwrap();
            store
                .create_session(&SessionMeta {
                    session_id: "raw".into(),
                    soul_id: String::new(),
                    user_id: "user".into(),
                    state: SessionState::Active as i32,
                    active_lease: String::new(),
                    created_at: Some(fabric_context::db::ms_to_timestamp(now_ms())),
                    last_activity: None,
                    labels: HashMap::new(),
                    org_id: String::new(),
                })
                .unwrap();
        }
        let err = append(&state, "raw", entry("e1", None)).unwrap_err();
        assert_eq!(err.code(), "invalid_session");

        // session.load makes the default holder the writer; appends pass.
        dispatch(&state, "session.load", &json!({ "id": "raw" })).unwrap();
        let ok = append(&state, "raw", entry("e1", None)).unwrap();
        assert_eq!(ok["seq"], 1);
    }

    #[test]
    fn append_rejects_foreign_lease_holder() {
        let state = test_state();
        let id = create_session(&state);
        let err = dispatch(
            &state,
            "entry.append",
            &json!({
                "session_id": id,
                "holder_id": "someone-else",
                "entry": entry("e1", None),
            }),
        )
        .unwrap_err();
        assert_eq!(err.code(), "invalid_session");
    }

    #[test]
    fn append_read_list_head_roundtrip() {
        let state = test_state();
        let id = create_session(&state);
        append(&state, &id, entry("e1", None)).unwrap();
        append(&state, &id, entry("e2", Some("e1"))).unwrap();

        let head = dispatch(&state, "session.head", &json!({ "session_id": id })).unwrap();
        assert_eq!(head["leaf_id"], "e2");

        let read = dispatch(
            &state,
            "entry.read",
            &json!({ "session_id": id, "id": "e1" }),
        )
        .unwrap();
        assert_eq!(read["seq"], 1);
        assert_eq!(read["entry"]["text"], "message e1");

        let missing = dispatch(
            &state,
            "entry.read",
            &json!({ "session_id": id, "id": "ghost" }),
        )
        .unwrap();
        assert!(missing["entry"].is_null());

        let list = dispatch(
            &state,
            "entry.list",
            &json!({ "session_id": id, "after_seq": 1, "limit": 10 }),
        )
        .unwrap();
        let entries = list["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["id"], "e2");
    }

    #[test]
    fn path_walks_parent_chain_leaf_first() {
        let state = test_state();
        let id = create_session(&state);
        append(&state, &id, entry("e1", None)).unwrap();
        append(&state, &id, entry("e2", Some("e1"))).unwrap();
        append(&state, &id, entry("e3", Some("e2"))).unwrap();

        let path = dispatch(
            &state,
            "entry.path",
            &json!({ "session_id": id, "leaf_id": "e3" }),
        )
        .unwrap();
        let ids: Vec<&str> = path["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["e3", "e2", "e1"]);

        let empty = dispatch(
            &state,
            "entry.path",
            &json!({ "session_id": id, "leaf_id": Value::Null }),
        )
        .unwrap();
        assert_eq!(empty["entries"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn fork_copies_entries_and_rewrites_ids() {
        let state = test_state();
        let source = create_session(&state);
        append(&state, &source, entry("e1", None)).unwrap();
        append(&state, &source, entry("e2", Some("e1"))).unwrap();
        append(&state, &source, entry("e3", Some("e2"))).unwrap();

        let forked = dispatch(
            &state,
            "session.fork",
            &json!({
                "source_id": source,
                "selection": { "kind": "through_entry", "entryId": "e2" },
            }),
        )
        .unwrap();
        let fork_id = forked["id"].as_str().unwrap();
        assert_eq!(forked["head_seq"], 2);

        let list = dispatch(&state, "entry.list", &json!({ "session_id": fork_id })).unwrap();
        let entries = list["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        // Fresh ids, parent chain remapped to them.
        assert_ne!(entries[0]["id"], "e1");
        assert_eq!(entries[1]["parentId"], entries[0]["id"]);

        // The fork is writable by the forking holder.
        let ok = append(&state, fork_id, entry("e4", None)).unwrap();
        assert_eq!(ok["seq"], 3);
    }

    #[test]
    fn fork_before_user_message_excludes_target() {
        let state = test_state();
        let source = create_session(&state);
        append(&state, &source, entry("e1", None)).unwrap();
        append(&state, &source, entry("e2", Some("e1"))).unwrap();

        let forked = dispatch(
            &state,
            "session.fork",
            &json!({
                "source_id": source,
                "selection": { "kind": "before_user_message", "entryId": "e2" },
            }),
        )
        .unwrap();
        assert_eq!(forked["head_seq"], 1);
    }

    #[test]
    fn fork_invalid_targets() {
        let state = test_state();
        let source = create_session(&state);
        append(&state, &source, entry("e1", None)).unwrap();

        let err = dispatch(
            &state,
            "session.fork",
            &json!({
                "source_id": source,
                "selection": { "kind": "through_entry", "entryId": "ghost" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code(), "invalid_fork_target");

        let err = dispatch(
            &state,
            "session.fork",
            &json!({
                "source_id": source,
                "selection": { "kind": "sideways" },
            }),
        )
        .unwrap_err();
        assert_eq!(err.code(), "invalid_fork_target");
    }

    #[test]
    fn append_runs_through_policy_gate() {
        let state = test_state();
        state
            .policy
            .write()
            .unwrap()
            .load_endpoint(EndpointPolicy {
                policy_id: "ep".into(),
                version: "v1".into(),
                org_id: "org".into(),
                dlp_patterns: vec![DlpPattern {
                    name: "private-key".into(),
                    regex: r"BEGIN [A-Z ]*PRIVATE KEY".into(),
                    action: DlpAction::Block as i32,
                }],
                ..Default::default()
            })
            .unwrap();
        let id = create_session(&state);

        // DLP block: the entry never lands in the log.
        let mut bad = entry("e1", None);
        bad["text"] = Value::String("-----BEGIN RSA PRIVATE KEY-----".into());
        let err = append(&state, &id, bad).unwrap_err();
        assert_eq!(err.code(), "invalid_entry");
        let head = dispatch(&state, "session.head", &json!({ "session_id": id })).unwrap();
        assert!(head["leaf_id"].is_null());

        // Clean entries pass the gate.
        append(&state, &id, entry("e2", None)).unwrap();

        // Kill switch denies everything.
        state
            .policy
            .write()
            .unwrap()
            .load_endpoint(EndpointPolicy {
                policy_id: "ep".into(),
                version: "v2".into(),
                org_id: "org".into(),
                kill_switch: true,
                ..Default::default()
            })
            .unwrap();
        let err = append(&state, &id, entry("e3", None)).unwrap_err();
        assert_eq!(err.code(), "invalid_entry");
        assert!(err.message().contains("kill switch"));
    }

    #[test]
    fn unknown_method_is_unknown_error() {
        let state = test_state();
        let err = dispatch(&state, "session.explode", &json!({})).unwrap_err();
        assert_eq!(err.code(), "unknown");
    }

    #[test]
    fn rfc3339_formats_epoch_ms() {
        assert_eq!(rfc3339_from_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339_from_ms(100_000), "1970-01-01T00:01:40.000Z");
        assert_eq!(
            rfc3339_from_ms(1_785_350_400_000),
            "2026-07-29T18:40:00.000Z"
        );
    }
}
