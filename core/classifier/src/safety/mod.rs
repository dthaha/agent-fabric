pub mod client;
#[cfg(feature = "safety-granite-guardian")]
pub mod granite_guardian;
#[cfg(feature = "safety-llama-guard")]
pub mod llama_guard;
#[cfg(feature = "safety-nemotron-cs")]
pub mod nemotron_cs;
pub mod policy;
#[cfg(feature = "safety-shield-gemma")]
pub mod shield_gemma;

pub use client::SafetyClient;
#[cfg(feature = "safety-nemotron-cs")]
pub use nemotron_cs::NemotronContentSafetyParser;
pub use policy::SafetyPolicyEnforcer;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyLevel {
    Safe,
    Unsafe,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyCategory {
    Violence,
    SexualContent,
    Pii,
    Financial,
    Injection,
    Profanity,
    SelfHarm,
    IllegalActivity,
    MinorSafety,
    Custom(String),
}

impl SafetyCategory {
    pub fn as_str(&self) -> &str {
        match self {
            SafetyCategory::Violence => "safety:violence",
            SafetyCategory::SexualContent => "safety:sexual_content",
            SafetyCategory::Pii => "safety:pii",
            SafetyCategory::Financial => "safety:financial",
            SafetyCategory::Injection => "safety:injection",
            SafetyCategory::Profanity => "safety:profanity",
            SafetyCategory::SelfHarm => "safety:self_harm",
            SafetyCategory::IllegalActivity => "safety:illegal_activity",
            SafetyCategory::MinorSafety => "safety:minor_safety",
            SafetyCategory::Custom(s) => s.as_str(),
        }
    }
}

/// The ONE canonical string→category mapping. Parsers, the policy engine,
/// and any other caller must go through this so a category detected by a
/// model always matches the same category in a policy rule. Matching is
/// case-insensitive and knows every first-class parser's aliases (Llama
/// Guard S-codes, Granite Guardian labels, ShieldGemma keys, Nemotron CS
/// category names). Unknown strings map to `Custom` with the original
/// (trimmed, case-preserved) text so custom policy rules match exactly.
pub fn parse_safety_category(s: &str) -> SafetyCategory {
    let trimmed = s.trim();
    let lower = trimmed.to_lowercase();
    let key = lower.strip_prefix("safety:").unwrap_or(lower.as_str());
    match key {
        "violence" | "harm" | "harassment" | "s1" => SafetyCategory::Violence,
        "sexual_content" | "sexual" | "sexual content" | "sexually_explicit"
        | "sexually explicit" | "s2" => SafetyCategory::SexualContent,
        "pii" | "pii/privacy" | "s6" => SafetyCategory::Pii,
        "financial" | "fraud/deception" => SafetyCategory::Financial,
        "injection" | "prompt_injection" | "prompt injection" | "malware/cybersecurity" => {
            SafetyCategory::Injection
        }
        "profanity" | "hate" | "toxic" | "hate_speech" | "hate speech" | "hatespeech" | "s4" => {
            SafetyCategory::Profanity
        }
        "self_harm" | "self-harm" | "s5" => SafetyCategory::SelfHarm,
        "illegal_activity"
        | "illegal"
        | "criminal"
        | "criminal planning/confessions"
        | "dangerous_content"
        | "dangerous content"
        | "dangerous"
        | "s3" => SafetyCategory::IllegalActivity,
        "minor_safety" | "minor" | "s7" => SafetyCategory::MinorSafety,
        _ => SafetyCategory::Custom(trimmed.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyAction {
    Block,
    ForceEndpoint,
    Warn,
    Allow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SafetyVerdict {
    pub verdict: SafetyLevel,
    pub categories: Vec<SafetyCategory>,
    pub explanation: Option<String>,
    pub model_id: String,
    pub raw_output: String,
}

impl SafetyVerdict {
    /// Fail-soft verdict for unparseable model output. Unknown is treated as
    /// unsafe by policy — the fail-closed philosophy.
    pub fn unknown(model_id: &str, raw_output: &str) -> Self {
        Self {
            verdict: SafetyLevel::Unknown,
            categories: Vec::new(),
            explanation: None,
            model_id: model_id.to_string(),
            raw_output: raw_output.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyEnforcement {
    pub action: SafetyAction,
    pub triggered_categories: Vec<SafetyCategory>,
    pub blocked: bool,
    pub force_endpoint: bool,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("failed to parse safety model output: {0}")]
    ParseError(String),
    #[error("unknown verdict: {0}")]
    UnknownVerdict(String),
    #[error("unknown category code: {0}")]
    UnknownCategory(String),
}

#[derive(Debug, Error)]
pub enum SafetyError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("timeout after {0}ms")]
    Timeout(u64),
}

impl From<reqwest::Error> for SafetyError {
    fn from(e: reqwest::Error) -> Self {
        SafetyError::Http(e.to_string())
    }
}

/// Output parser for a content-safety model: maps raw model output to a
/// canonical [`SafetyVerdict`].
///
/// Fail semantics (fail-closed philosophy — `Unknown` is treated as unsafe
/// by policy):
/// - `parse()` returns `Ok(SafetyVerdict)` with [`SafetyLevel::Unknown`]
///   when the model output is unparseable garbage (fail-soft).
/// - `parse()` returns `Err(ParseError)` only for structural failures (e.g.
///   missing required fields in a well-formed response).
pub trait SafetyParser: Send + Sync {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError>;
    fn name(&self) -> &str;
    /// Default system prompt for this model, used when the policy pack
    /// leaves `SafetyConfig.system_prompt` empty. Empty means "send no
    /// system message" (models with their own chat template).
    fn default_system_prompt(&self) -> &str {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_safety_category_canonical_names() {
        assert_eq!(parse_safety_category("violence"), SafetyCategory::Violence);
        assert_eq!(parse_safety_category("safety:pii"), SafetyCategory::Pii);
        assert_eq!(
            parse_safety_category("Minor_Safety"),
            SafetyCategory::MinorSafety
        );
    }

    #[test]
    fn parse_safety_category_aliases_across_parsers() {
        assert_eq!(parse_safety_category("harm"), SafetyCategory::Violence);
        assert_eq!(parse_safety_category("S3"), SafetyCategory::IllegalActivity);
        assert_eq!(
            parse_safety_category("Hate Speech"),
            SafetyCategory::Profanity
        );
        assert_eq!(
            parse_safety_category("dangerous_content"),
            SafetyCategory::IllegalActivity
        );
        assert_eq!(
            parse_safety_category("Criminal Planning/Confessions"),
            SafetyCategory::IllegalActivity
        );
        assert_eq!(
            parse_safety_category("prompt injection"),
            SafetyCategory::Injection
        );
    }

    #[test]
    fn parse_safety_category_unknown_preserves_case() {
        assert_eq!(
            parse_safety_category("Something New"),
            SafetyCategory::Custom("Something New".to_string())
        );
    }

    #[test]
    fn default_system_prompt_is_empty() {
        struct Stub;
        impl SafetyParser for Stub {
            fn parse(&self, _raw: &str, _model: &str) -> Result<SafetyVerdict, ParseError> {
                Ok(SafetyVerdict::unknown(_model, _raw))
            }
            fn name(&self) -> &str {
                "stub"
            }
        }
        assert_eq!(Stub.default_system_prompt(), "");
    }
}
