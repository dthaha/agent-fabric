//! Offline locus classifier. Decides where each turn thinks (endpoint,
//! server, split) entirely on-device — it never calls home to decide where
//! to think.
//!
//! The architecture is three layers:
//!
//! 1. **Model** (Phase 5): a small on-device model produces a
//!    [`ModelAdvisory`] — a semantic estimate of complexity, horizon, and
//!    suggested locus. The model informs; it never decides.
//! 2. **Rules** ([`RulesClassifier`]): the decision layer. Hard constraints
//!    (kill switch, user prefs, network, restricted data, endpoint-only
//!    tools) always win. When an advisory is present its suggestion is used
//!    as the semantic estimate; otherwise deterministic heuristics fill in.
//! 3. **Policy** ([`PolicyAwareClassifier`]): the final veto. Every
//!    off-device decision is re-checked against the effective policy and
//!    downgraded to the endpoint when the gate would refuse it.

use serde::{Deserialize, Serialize};

use fabric_types::context::Locus;

pub mod policy;
pub mod rules;
pub mod safety;

pub use policy::PolicyAwareClassifier;
pub use rules::RulesClassifier;

/// Decides where a single agent turn executes. Implementations must run
/// entirely on-device and never perform network I/O.
pub trait LocusClassifier: Send + Sync {
    fn classify(&self, input: &ClassifyInput) -> LocusDecision;
}

/// Rough size of the reasoning a turn needs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    #[default]
    Low,
    Medium,
    High,
}

/// How long the work is expected to run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Horizon {
    #[default]
    SingleTurn,
    MultiTurn,
    LongHorizon,
}

/// Explicit user steering for where the turn should run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserLocusPref {
    PreferLocal,
    PreferServer,
    Background,
    #[default]
    NoPreference,
}

/// Advisory output from a small on-device classifier model. When present,
/// the rules engine uses this as the semantic estimate instead of its own
/// heuristic fallbacks. The model suggests; the rules decide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelAdvisory {
    /// The model's suggested locus for this turn.
    pub suggested_locus: Locus,
    /// Semantic complexity estimate from the model.
    pub complexity: Complexity,
    /// Semantic horizon estimate from the model.
    pub horizon: Horizon,
    /// Data classes the model detected in the intent (e.g. PII, financial).
    /// These are ADDITIVE — they merge with any data_classes already on the
    /// input. The model can flag data the caller missed, but cannot remove
    /// flags the caller set.
    pub detected_data_classes: Vec<String>,
    /// Model confidence in its suggestion (0.0-1.0).
    pub confidence: f32,
}

/// Everything the classifier knows about a turn when deciding its locus.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassifyInput {
    pub intent_text: String,
    pub required_tools: Vec<String>,
    pub estimated_complexity: Complexity,
    pub estimated_horizon: Horizon,
    /// Data classes touched by this turn (e.g. "secret", "internal",
    /// "public"). Any restricted class pins the turn to the endpoint.
    pub data_classes: Vec<String>,
    pub network_available: bool,
    pub local_model_available: bool,
    pub user_preference: UserLocusPref,
    /// Advisory from the on-device classifier model. When None, the rules
    /// engine falls back to its own heuristic rules (long horizon → Split,
    /// high complexity → Server, etc.). When Some, the model's suggestion
    /// is used as the semantic estimate, subject to constraint validation.
    pub model_advisory: Option<ModelAdvisory>,
}

/// Where the turn will run, why, and where to fall back to if the chosen
/// locus becomes unavailable mid-turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocusDecision {
    pub locus: Locus,
    pub reason: String,
    /// 0.0 (guess) to 1.0 (forced).
    pub confidence: f32,
    pub fallback: Option<Locus>,
}
