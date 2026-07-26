//! Encode/decode helpers for the structured [`ToolCall`] payload carried by
//! `ENTRY_KIND_TOOL_CALL` entries. The entry's opaque `payload` bytes hold a
//! prost-encoded `ToolCall`; entries whose payloads do not decode (legacy or
//! foreign writers) are treated as opaque by the conflict detector.

use fabric_types::context::ToolCall;
use prost::Message;

/// Encode a [`ToolCall`] into `ContextEntry.payload` bytes.
pub fn encode(call: &ToolCall) -> Vec<u8> {
    call.encode_to_vec()
}

/// Decode a [`ToolCall`] from `ContextEntry.payload` bytes.
pub fn decode(payload: &[u8]) -> Result<ToolCall, prost::DecodeError> {
    ToolCall::decode(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn round_trip() {
        let call = ToolCall {
            tool_name: "set_config".into(),
            target: "ui.theme".into(),
            params: HashMap::from([("value".into(), "dark".into())]),
            idempotency_key: "req-1".into(),
        };
        let bytes = encode(&call);
        assert_eq!(decode(&bytes).unwrap(), call);
    }

    #[test]
    fn rejects_opaque_payload() {
        assert!(decode(b"hello").is_err());
    }
}
