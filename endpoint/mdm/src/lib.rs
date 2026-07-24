//! Endpoint MDM ingest: parses Intune/Jamf policy packs delivered to the
//! device and loads them as the endpoint policy ceiling. The endpoint can
//! tighten this ceiling locally but never loosen it.
//!
//! Wire format: JSON. Either a bare `EndpointPolicy` document, or the
//! MDM-delivered wrapper:
//!
//! ```json
//! {"format":"fabric-mdm/v1","policy":{...},"signature":"..."}
//! ```
//!
//! The wrapper `signature` is a placeholder for future code-signing
//! verification and is currently ignored.

use std::path::Path;

use fabric_types::policy::EndpointPolicy;
use serde::Deserialize;
use thiserror::Error;

/// Format marker prefix carried by MDM wrapper documents.
pub const PACK_FORMAT_PREFIX: &str = "fabric-mdm/";

#[derive(Debug, Error)]
pub enum MdmError {
    #[error("reading policy pack {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("parsing policy pack: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported policy pack format: {0}")]
    UnsupportedFormat(String),
    #[error("invalid policy pack: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, MdmError>;

/// MDM wrapper document. The `format` marker is checked on the raw value
/// before this struct is deserialized, so only the payload is modeled here.
#[derive(Debug, Deserialize)]
struct PolicyPack {
    policy: serde_json::Value,
    /// Placeholder for future code-signing verification. Ignored for now.
    #[serde(default)]
    #[allow(dead_code)]
    signature: Option<String>,
}

/// True when `bytes` look like an MDM wrapper document (a JSON object with
/// a `format` field carrying the fabric-mdm marker).
pub fn is_policy_pack(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v.get("format")?.as_str().map(str::to_owned))
        .is_some_and(|f| f.starts_with(PACK_FORMAT_PREFIX))
}

/// Parse an MDM policy pack from JSON bytes. Accepts the wrapper format
/// (`fabric-mdm/v1`) or a bare `EndpointPolicy` document. The resulting
/// policy is validated: `policy_id`, `version`, and `org_id` must all be
/// non-empty.
pub fn parse_policy_pack(bytes: &[u8]) -> Result<EndpointPolicy> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let policy_value = if let Some(format) = value.get("format").and_then(|f| f.as_str()) {
        if !format.starts_with(PACK_FORMAT_PREFIX) {
            return Err(MdmError::UnsupportedFormat(format.to_string()));
        }
        let pack: PolicyPack = serde_json::from_value(value)?;
        pack.policy
    } else {
        value
    };
    let policy: EndpointPolicy = serde_json::from_value(policy_value)?;
    validate(&policy)?;
    Ok(policy)
}

/// Read a policy pack from `path`, parse it, and validate it.
pub fn load_mdm_policy(path: &Path) -> Result<EndpointPolicy> {
    let bytes = std::fs::read(path).map_err(|source| MdmError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_policy_pack(&bytes)
}

fn validate(policy: &EndpointPolicy) -> Result<()> {
    for (field, value) in [
        ("policy_id", &policy.policy_id),
        ("version", &policy.version),
        ("org_id", &policy.org_id),
    ] {
        if value.is_empty() {
            return Err(MdmError::Validation(format!("{field} must be non-empty")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare_policy_json() -> serde_json::Value {
        serde_json::json!({
            "policyId": "ep-1",
            "version": "v1",
            "orgId": "org-1",
            "toolRules": [
                {"toolPattern": "fs.read", "action": "TOOL_ACTION_ALLOW"}
            ],
        })
    }

    #[test]
    fn parses_valid_wrapper() {
        let wrapper = serde_json::json!({
            "format": "fabric-mdm/v1",
            "policy": bare_policy_json(),
            "signature": "deadbeef",
        });
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        assert!(is_policy_pack(&bytes));

        let policy = parse_policy_pack(&bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-1");
        assert_eq!(policy.version, "v1");
        assert_eq!(policy.org_id, "org-1");
        assert_eq!(policy.tool_rules.len(), 1);
    }

    #[test]
    fn parses_bare_policy() {
        let bytes = serde_json::to_vec(&bare_policy_json()).unwrap();
        assert!(!is_policy_pack(&bytes));

        let policy = parse_policy_pack(&bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-1");
    }

    #[test]
    fn missing_policy_id_fails_validation() {
        let mut doc = bare_policy_json();
        doc["policyId"] = serde_json::json!("");
        let err = parse_policy_pack(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
        assert!(matches!(err, MdmError::Validation(ref m) if m.contains("policy_id")));
    }

    #[test]
    fn missing_version_and_org_fail_validation() {
        for field in ["version", "orgId"] {
            let mut doc = bare_policy_json();
            doc[field] = serde_json::json!("");
            let err = parse_policy_pack(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
            assert!(matches!(err, MdmError::Validation(_)), "field {field}");
        }
    }

    #[test]
    fn corrupt_json_fails() {
        let err = parse_policy_pack(b"{not json").unwrap_err();
        assert!(matches!(err, MdmError::Json(_)));
    }

    #[test]
    fn unknown_format_marker_fails() {
        let wrapper = serde_json::json!({
            "format": "other/v9",
            "policy": bare_policy_json(),
        });
        let err = parse_policy_pack(&serde_json::to_vec(&wrapper).unwrap()).unwrap_err();
        assert!(matches!(err, MdmError::UnsupportedFormat(_)));
    }

    #[test]
    fn load_mdm_policy_reads_file() {
        let path =
            std::env::temp_dir().join(format!("fabric-mdm-pack-{}.json", std::process::id()));
        std::fs::write(&path, serde_json::to_vec(&bare_policy_json()).unwrap()).unwrap();

        let policy = load_mdm_policy(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(policy.policy_id, "ep-1");

        assert!(matches!(
            load_mdm_policy(Path::new("/nonexistent/pack.json")),
            Err(MdmError::Io { .. })
        ));
    }
}
