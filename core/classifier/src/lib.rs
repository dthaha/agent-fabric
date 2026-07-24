//! Offline locus classifier. Decides where each turn thinks (endpoint,
//! hosted, split) using on-device rules plus an optional tiny model — it
//! never calls home to decide where to think.

use serde::{Deserialize, Serialize};

use fabric_types::context::Locus;

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
    PreferHosted,
    Background,
    #[default]
    NoPreference,
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
