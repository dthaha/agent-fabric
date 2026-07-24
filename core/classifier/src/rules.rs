//! Deterministic rules engine. The decision matrix is evaluated in order and
//! the first matching rule wins, so the most forced outcomes (kill switch,
//! no network, restricted data) always beat the softer heuristics
//! (complexity, horizon, local model availability).

use fabric_types::context::Locus;

use crate::{ClassifyInput, Complexity, Horizon, LocusClassifier, LocusDecision, UserLocusPref};

/// Rules-based locus classifier. Pure, synchronous, and offline: it only
/// looks at the [`ClassifyInput`] and its own static configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesClassifier {
    /// When true, everything is pinned to the endpoint. The policy gate
    /// denies all privileged acts while killed; the classifier mirrors that
    /// by never sending work off-device.
    pub policy_killed: bool,
    /// Data classes that must never leave the device.
    pub restricted_data_classes: Vec<String>,
    /// Tools that can only execute on the endpoint (e.g. CUA actuators).
    /// When every required tool is endpoint-only, the turn stays local.
    pub endpoint_only_tools: Vec<String>,
}

impl Default for RulesClassifier {
    fn default() -> Self {
        Self {
            policy_killed: false,
            restricted_data_classes: vec!["secret".into(), "restricted".into(), "pii".into()],
            endpoint_only_tools: Vec::new(),
        }
    }
}

impl RulesClassifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        policy_killed: bool,
        restricted_data_classes: Vec<String>,
        endpoint_only_tools: Vec<String>,
    ) -> Self {
        Self {
            policy_killed,
            restricted_data_classes,
            endpoint_only_tools,
        }
    }

    fn has_restricted_data<'a>(&self, input: &'a ClassifyInput) -> Option<&'a str> {
        input
            .data_classes
            .iter()
            .find(|c| self.restricted_data_classes.contains(c))
            .map(String::as_str)
    }

    fn all_tools_endpoint_only(&self, input: &ClassifyInput) -> bool {
        !input.required_tools.is_empty()
            && input
                .required_tools
                .iter()
                .all(|t| self.endpoint_only_tools.contains(t))
    }
}

impl LocusClassifier for RulesClassifier {
    fn classify(&self, input: &ClassifyInput) -> LocusDecision {
        // 1. Kill switch: the gate denies everything anyway; stay local.
        if self.policy_killed {
            return decision(Locus::Endpoint, "kill switch — local only", 1.0, None);
        }
        // 2. Background execution requested.
        if input.user_preference == UserLocusPref::Background && input.network_available {
            return decision(
                Locus::Hosted,
                "user requested background execution",
                0.95,
                Some(Locus::Endpoint),
            );
        }
        // 3. Explicit hosted preference.
        if input.user_preference == UserLocusPref::PreferHosted && input.network_available {
            return decision(
                Locus::Hosted,
                "user prefers hosted",
                0.9,
                Some(Locus::Endpoint),
            );
        }
        // 4. Explicit local preference (honoured even with network up).
        if input.user_preference == UserLocusPref::PreferLocal {
            return decision(Locus::Endpoint, "user prefers local", 0.95, None);
        }
        // 5. No network: forced local.
        if !input.network_available {
            return decision(Locus::Endpoint, "no network — forced local", 1.0, None);
        }
        // 6. Endpoint-only tools: the hands are here, so the loop stays here.
        if self.all_tools_endpoint_only(input) {
            return decision(
                Locus::Endpoint,
                "all required tools are endpoint-only",
                0.9,
                None,
            );
        }
        // 7. Restricted data may not leave the device.
        if let Some(class) = self.has_restricted_data(input) {
            return decision(
                Locus::Endpoint,
                format!("data class '{class}' must not leave the device"),
                0.95,
                None,
            );
        }
        // 8. Long-horizon work: hosted brain, endpoint hands via the bridge.
        if input.estimated_horizon == Horizon::LongHorizon && input.network_available {
            return decision(
                Locus::Split,
                "long-horizon task — hosted inference, endpoint tools",
                0.85,
                Some(Locus::Endpoint),
            );
        }
        // 9. Nothing to think with locally.
        if !input.local_model_available && input.network_available {
            return decision(
                Locus::Hosted,
                "no local model available",
                0.8,
                Some(Locus::Endpoint),
            );
        }
        // 10. Heavy reasoning favours hosted inference.
        if input.estimated_complexity == Complexity::High && input.network_available {
            return decision(
                Locus::Hosted,
                "high complexity — hosted inference",
                0.7,
                Some(Locus::Endpoint),
            );
        }
        // 11. Default: think where the tools live.
        decision(Locus::Endpoint, "default to endpoint", 0.6, None)
    }
}

fn decision(locus: Locus, reason: impl Into<String>, confidence: f32, fallback: Option<Locus>) -> LocusDecision {
    LocusDecision {
        locus,
        reason: reason.into(),
        confidence,
        fallback,
    }
}
