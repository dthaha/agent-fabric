//! Unix domain socket control interface for agent harnesses (ADR 008).
//!
//! Speaks NDJSON (one JSON object per line) over a local Unix socket.
//! Each connection gets its own tokio task; requests are dispatched
//! synchronously into the SQLite context store via `control_dispatch`.
//!
//! The socket is LOCAL ONLY — no network exposure. Filesystem permissions
//! on the socket path are the access control boundary.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::control_dispatch::{self, ControlError};
use crate::state::DaemonState;

/// Default socket path when FABRIC_SOCKET_PATH is unset.
const DEFAULT_SOCKET_PATH: &str = "/tmp/fabric-endpoint.sock";

/// Resolve the control socket path from the environment.
pub fn socket_path() -> PathBuf {
    std::env::var("FABRIC_SOCKET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SOCKET_PATH))
}

/// Run the control socket accept loop until `cancel` is triggered.
///
/// Binds a `UnixListener` at `path`, accepts connections, and spawns a
/// per-connection task that reads NDJSON lines, dispatches them, and
/// writes responses. Stale socket files from a previous run are removed
/// before binding.
pub async fn serve(
    state: Arc<DaemonState>,
    path: PathBuf,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    // Remove stale socket from a previous run (the daemon may have been
    // killed without cleanup). Ignore errors — if it doesn't exist, bind
    // will succeed; if we can't remove it, bind will fail with a clear
    // error.
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)
        .map_err(|e| anyhow::anyhow!("binding control socket {}: {e}", path.display()))?;

    // Best-effort: make the socket accessible to the user's agent
    // processes. 0o770 lets group members connect (e.g. a pi process
    // running under the same user group).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o770)) {
            warn!(path = %path.display(), error = %e, "failed to set socket permissions");
        }
    }

    info!(path = %path.display(), "control socket listening");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("control socket shutting down");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let state = Arc::clone(&state);
                        let cancel = cancel.clone();
                        tokio::spawn(handle_connection(stream, state, cancel));
                    }
                    Err(e) => {
                        warn!(error = %e, "control socket accept failed");
                    }
                }
            }
        }
    }

    // Clean up the socket file on graceful shutdown.
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Handle a single client connection: read NDJSON lines, dispatch, respond.
async fn handle_connection(stream: UnixStream, state: Arc<DaemonState>, cancel: CancellationToken) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    debug!("control socket client connected");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let (mut response, req_id) = process_line(&state, &line);
                        // Echo the request id so harness clients can
                        // correlate responses with requests.
                        if let Some(req_id) = req_id {
                            if let Some(obj) = response.as_object_mut() {
                                obj.insert("id".to_string(), req_id);
                            }
                        }
                        let mut out = serde_json::to_string(&response)
                            .unwrap_or_else(|_| r#"{"ok":false,"error":{"code":"unknown","message":"serialization failure"}}"#.to_string());
                        out.push('\n');
                        if writer.write_all(out.as_bytes()).await.is_err() {
                            break; // client disconnected
                        }
                    }
                    Ok(None) => break, // EOF
                    Err(e) => {
                        warn!(error = %e, "control socket read error");
                        break;
                    }
                }
            }
        }
    }

    debug!("control socket client disconnected");
}

/// Parse one NDJSON line and dispatch it. Never panics — all errors
/// become well-formed error responses.
///
/// Returns the response object plus the request id (when one was
/// present and extractable) so the caller can echo it into the NDJSON
/// line for client-side correlation.
fn process_line(state: &DaemonState, line: &str) -> (Value, Option<Value>) {
    match parse_line(line) {
        Ok((req_id, method, params)) => {
            let response = match control_dispatch::dispatch(state, &method, &params) {
                Ok(result) => json!({ "ok": true, "result": result }),
                Err(e) => error_response(&e),
            };
            (response, req_id)
        }
        Err((response, req_id)) => (response, req_id),
    }
}

/// Parsed request: request id (if any), method, params.
type ParsedRequest = (Option<Value>, String, Value);

/// Parse failure: error response plus whatever id was extracted before
/// the failure.
type ParseFailure = (Value, Option<Value>);

/// Parse and validate one NDJSON request line. On success returns the
/// request id (if any), method, and params. On failure returns the
/// error response plus whatever id could be extracted before the
/// failure.
fn parse_line(line: &str) -> Result<ParsedRequest, ParseFailure> {
    let line = line.trim();
    if line.is_empty() {
        return Err((
            error_response(&ControlError::Unknown("empty line".into())),
            None,
        ));
    }

    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Err((
                error_response(&ControlError::Unknown(format!("invalid JSON: {e}"))),
                None,
            ));
        }
    };

    // Extract the id as early as possible so it can be echoed even in
    // error responses for malformed requests.
    let req_id = request.get("id").cloned();

    let method = match request.get("method").and_then(|m| m.as_str()) {
        Some(m) => m.to_string(),
        None => {
            return Err((
                error_response(&ControlError::Unknown(
                    "missing or non-string 'method' field".into(),
                )),
                req_id,
            ));
        }
    };

    let params = request
        .get("params")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    Ok((req_id, method, params))
}

/// Build a well-formed NDJSON error response.
fn error_response(err: &ControlError) -> Value {
    json!({
        "ok": false,
        "error": {
            "code": err.code(),
            "message": err.message(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests exercise parse_line + response assembly only, since
    // dispatching needs a real DaemonState (SQLite store).
    // (Full integration tests are in tests/control_socket.rs.)

    #[test]
    fn parse_line_invalid_json() {
        let (resp, req_id) = parse_line("not json at all").unwrap_err();
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["error"]["code"], "unknown");
        assert!(req_id.is_none());
    }

    #[test]
    fn parse_line_missing_method() {
        let (resp, req_id) = parse_line(r#"{"id": "req-1", "params": {}}"#).unwrap_err();
        assert_eq!(resp["ok"], false);
        assert_eq!(resp["error"]["code"], "unknown");
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("method"));
        // The id was parsed before the failure, so it can be echoed.
        assert_eq!(req_id, Some(json!("req-1")));
    }

    #[test]
    fn parse_line_empty() {
        let (resp, req_id) = parse_line("").unwrap_err();
        assert_eq!(resp["ok"], false);
        assert!(req_id.is_none());
    }

    #[test]
    fn parse_line_extracts_id_method_params() {
        let (req_id, method, params) =
            parse_line(r#"{"id":"abc-123","method":"session.open","params":{"a":1}}"#).unwrap();
        assert_eq!(req_id, Some(json!("abc-123")));
        assert_eq!(method, "session.open");
        assert_eq!(params, json!({ "a": 1 }));
    }

    #[test]
    fn parse_line_defaults_missing_id_and_params() {
        let (req_id, method, params) = parse_line(r#"{"method":"session.open"}"#).unwrap();
        assert!(req_id.is_none());
        assert_eq!(method, "session.open");
        assert_eq!(params, json!({}));
    }

    #[test]
    fn parse_line_non_string_id_echoed_verbatim() {
        let (req_id, ..) = parse_line(r#"{"id":42,"method":"session.open"}"#).unwrap();
        assert_eq!(req_id, Some(json!(42)));
    }
}
