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

pub trait SafetyParser: Send + Sync {
    fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError>;
    fn name(&self) -> &str;
}
