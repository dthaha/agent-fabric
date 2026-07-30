//! Endpoint MDM ingest: parses policy packs delivered to the device in
//! MDM-native formats and loads them as the endpoint policy ceiling. The
//! endpoint can tighten this ceiling locally but never loosen it.
//!
//! Formats (ADR 005): Jamf plist Configuration Profiles (macOS), Intune
//! OMA-URI XML (Windows), and the generic `fabric-mdm/v1` JSON wrapper or
//! a bare `EndpointPolicy` JSON document (Linux / generic MDM). The format
//! is auto-detected from the payload bytes.
//!
//! Per ADR 005 there is no application-layer signing: the MDM channel is
//! the trust anchor. JSON documents carrying a legacy `signature` key are
//! still accepted; the key is ignored.

use std::path::Path;

use fabric_types::policy::EndpointPolicy;
use serde::Deserialize;
use thiserror::Error;

mod oma_uri;
mod plist_parser;

/// Format marker prefix carried by MDM wrapper documents.
pub const PACK_FORMAT_PREFIX: &str = "fabric-mdm/";

/// Maximum accepted policy pack size (1 MiB). A policy document is a few
/// KiB; anything larger is a corrupt or hostile payload and must not be
/// parsed (XML/JSON parsers are not memory-bounded on adversarial input).
pub const MAX_PACK_SIZE: usize = 1_048_576;

/// How many leading bytes `detect_format` scans for the `<plist` marker.
/// Plist documents can carry long XML comments or payload declarations
/// before the root element; 4 KiB is not always enough.
const PLIST_SNIFF_BYTES: usize = 65_536;

#[derive(Debug, Error)]
pub enum MdmError {
    #[error("reading policy pack {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("parsing JSON policy pack: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parsing plist policy pack: {0}")]
    Plist(#[from] plist::Error),
    #[error("parsing OMA-URI policy pack: {0}")]
    OmaUri(#[from] quick_xml::DeError),
    #[error("unsupported policy pack format: {0}")]
    UnsupportedFormat(String),
    #[error("invalid policy pack: {0}")]
    Validation(String),
    #[error("policy pack too large: {0} bytes (max {MAX_PACK_SIZE})")]
    PackTooLarge(usize),
}

pub type Result<T> = std::result::Result<T, MdmError>;

/// Policy pack wire format, per ADR 005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackFormat {
    /// `fabric-mdm/v1` JSON wrapper or bare `EndpointPolicy` JSON.
    Json,
    /// Jamf Configuration Profile plist (macOS).
    Plist,
    /// Intune OMA-URI `FabricPolicy` XML (Windows).
    OmaUri,
}

/// MDM wrapper document. The `format` marker is checked on the raw value
/// before this struct is deserialized, so only the payload is modeled here.
#[derive(Debug, Deserialize)]
struct PolicyPack {
    policy: serde_json::Value,
}

/// True when `bytes` look like an MDM wrapper document (a JSON object with
/// a `format` field carrying the fabric-mdm marker).
pub fn is_policy_pack(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v.get("format")?.as_str().map(str::to_owned))
        .is_some_and(|f| f.starts_with(PACK_FORMAT_PREFIX))
}

/// Detect the wire format of a policy pack from its leading bytes: XML
/// documents containing a `<plist` element are Jamf plists, other XML
/// documents are treated as Intune OMA-URI, and everything else is JSON.
/// The `<plist` marker is searched within the first [`PLIST_SNIFF_BYTES`]
/// so leading XML comments cannot hide it.
pub fn detect_format(bytes: &[u8]) -> PackFormat {
    let text = std::str::from_utf8(bytes).unwrap_or("");
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('<') {
        let head = trimmed.get(..PLIST_SNIFF_BYTES).unwrap_or(trimmed);
        if head.contains("<plist") {
            PackFormat::Plist
        } else {
            PackFormat::OmaUri
        }
    } else {
        PackFormat::Json
    }
}

/// Parse an MDM policy pack using an explicit wire format. The resulting
/// policy is validated: `policy_id`, `version`, and `org_id` must all be
/// non-empty. Packs larger than [`MAX_PACK_SIZE`] are rejected before any
/// parsing happens.
pub fn parse_with_format(bytes: &[u8], format: PackFormat) -> Result<EndpointPolicy> {
    if bytes.len() > MAX_PACK_SIZE {
        return Err(MdmError::PackTooLarge(bytes.len()));
    }
    let policy = match format {
        PackFormat::Json => parse_json(bytes)?,
        PackFormat::Plist => plist_parser::parse(bytes)?,
        PackFormat::OmaUri => oma_uri::parse(bytes)?,
    };
    validate(&policy)?;
    Ok(policy)
}

/// Parse an MDM policy pack, auto-detecting the wire format from the
/// payload bytes. See [`parse_with_format`].
pub fn parse_policy_pack(bytes: &[u8]) -> Result<EndpointPolicy> {
    parse_with_format(bytes, detect_format(bytes))
}

/// Read a policy pack from `path`, parse it, and validate it.
pub fn load_mdm_policy(path: &Path) -> Result<EndpointPolicy> {
    let bytes = std::fs::read(path).map_err(|source| MdmError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_policy_pack(&bytes)
}

fn parse_json(bytes: &[u8]) -> Result<EndpointPolicy> {
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
    Ok(serde_json::from_value(policy_value)?)
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
    fn legacy_signature_key_is_ignored() {
        let wrapper = serde_json::json!({
            "format": "fabric-mdm/v1",
            "policy": bare_policy_json(),
            "signature": "deadbeef",
        });
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        assert!(is_policy_pack(&bytes));

        let policy = parse_policy_pack(&bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-1");
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

    #[test]
    fn oversized_pack_is_rejected_before_parsing() {
        let mut bytes = b"{\"policyId\":\"ep-1\"".to_vec();
        bytes.resize(MAX_PACK_SIZE + 1, b' ');
        let err = parse_policy_pack(&bytes).unwrap_err();
        assert!(matches!(err, MdmError::PackTooLarge(n) if n == MAX_PACK_SIZE + 1));

        // Explicit-format parsing is guarded too.
        let err = parse_with_format(&bytes, PackFormat::Json).unwrap_err();
        assert!(matches!(err, MdmError::PackTooLarge(_)));

        // Exactly at the cap is accepted for parsing (trailing whitespace
        // is valid JSON).
        let mut at_cap = serde_json::to_vec(&bare_policy_json()).unwrap();
        assert!(at_cap.len() < MAX_PACK_SIZE);
        at_cap.resize(MAX_PACK_SIZE, b' ');
        let policy = parse_policy_pack(&at_cap).unwrap();
        assert_eq!(policy.policy_id, "ep-1");
    }

    #[test]
    fn plist_marker_after_long_leading_comment_is_detected() {
        // >4 KiB of XML comment before the root element must not hide the
        // <plist marker (the sniff window covers 64 KiB).
        let comment = "x".repeat(8_000);
        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- {comment} -->\n\
             <plist version=\"1.0\">\n<dict>\n\
             \x20   <key>PolicyID</key>\n    <string>ep-mac</string>\n\
             \x20   <key>Version</key>\n    <string>v1</string>\n\
             \x20   <key>OrgID</key>\n    <string>org-1</string>\n\
             </dict>\n</plist>\n"
        );
        let bytes = doc.as_bytes();
        assert_eq!(detect_format(bytes), PackFormat::Plist);
        let policy = parse_policy_pack(bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-mac");
    }

    #[test]
    fn detects_json_format() {
        assert_eq!(detect_format(b"{\"policyId\":\"ep-1\"}"), PackFormat::Json);
        assert_eq!(detect_format(b"  \n{\"a\":1}"), PackFormat::Json);
    }

    #[test]
    fn detects_plist_format() {
        let bytes = b"<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict></dict></plist>";
        assert_eq!(detect_format(bytes), PackFormat::Plist);
    }

    #[test]
    fn detects_oma_uri_format() {
        let bytes = b"<?xml version=\"1.0\"?>\n<FabricPolicy></FabricPolicy>";
        assert_eq!(detect_format(bytes), PackFormat::OmaUri);
        assert_eq!(
            detect_format(b"<FabricPolicy></FabricPolicy>"),
            PackFormat::OmaUri
        );
    }

    #[test]
    fn auto_detects_plist_pack() {
        let bytes = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>PolicyID</key>
    <string>ep-mac</string>
    <key>Version</key>
    <string>v1</string>
    <key>OrgID</key>
    <string>org-1</string>
</dict>
</plist>
"#;
        assert!(!is_policy_pack(bytes));
        let policy = parse_policy_pack(bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-mac");
    }

    #[test]
    fn auto_detects_oma_uri_pack() {
        let bytes = br#"<FabricPolicy>
  <PolicyID>ep-win</PolicyID>
  <Version>v1</Version>
  <OrgID>org-1</OrgID>
</FabricPolicy>"#;
        let policy = parse_policy_pack(bytes).unwrap();
        assert_eq!(policy.policy_id, "ep-win");
    }

    #[test]
    fn explicit_format_selection() {
        let bytes = serde_json::to_vec(&bare_policy_json()).unwrap();
        let policy = parse_with_format(&bytes, PackFormat::Json).unwrap();
        assert_eq!(policy.policy_id, "ep-1");

        let err = parse_with_format(&bytes, PackFormat::OmaUri).unwrap_err();
        assert!(matches!(err, MdmError::OmaUri(_)));
    }
}
