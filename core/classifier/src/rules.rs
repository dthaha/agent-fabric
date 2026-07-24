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

fn decision(
    locus: Locus,
    reason: impl Into<String>,
    confidence: f32,
    fallback: Option<Locus>,
) -> LocusDecision {
    LocusDecision {
        locus,
        reason: reason.into(),
        confidence,
        fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ClassifyInput {
        ClassifyInput {
            intent_text: "summarize my emails".into(),
            required_tools: vec![],
            estimated_complexity: Complexity::Low,
            estimated_horizon: Horizon::SingleTurn,
            data_classes: vec!["public".into()],
            network_available: true,
            local_model_available: true,
            user_preference: UserLocusPref::NoPreference,
        }
    }

    #[test]
    fn kill_switch_forces_endpoint() {
        let c = RulesClassifier::with_config(true, vec![], vec![]);
        let mut i = input();
        i.user_preference = UserLocusPref::PreferHosted;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 1.0);
        assert_eq!(d.fallback, None);
    }

    #[test]
    fn background_preference_goes_hosted() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.user_preference = UserLocusPref::Background;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Hosted);
        assert_eq!(d.confidence, 0.95);
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn prefer_hosted_goes_hosted() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.user_preference = UserLocusPref::PreferHosted;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Hosted);
        assert_eq!(d.confidence, 0.9);
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn prefer_local_stays_endpoint() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.user_preference = UserLocusPref::PreferLocal;
        i.estimated_complexity = Complexity::High;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 0.95);
    }

    #[test]
    fn no_network_forces_endpoint_even_when_prefer_hosted() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.network_available = false;
        i.user_preference = UserLocusPref::PreferHosted;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 1.0);
        assert_eq!(d.fallback, None);
    }

    #[test]
    fn background_without_network_falls_through_to_forced_local() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.network_available = false;
        i.user_preference = UserLocusPref::Background;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn restricted_data_class_stays_endpoint() {
        let c = RulesClassifier::new();
        for class in ["secret", "restricted", "pii"] {
            let mut i = input();
            i.data_classes = vec!["public".into(), class.into()];
            i.estimated_complexity = Complexity::High;
            let d = c.classify(&i);
            assert_eq!(d.locus, Locus::Endpoint, "class {class}");
            assert_eq!(d.confidence, 0.95);
            assert!(d.reason.contains(class));
        }
    }

    #[test]
    fn custom_restricted_data_classes() {
        let c = RulesClassifier::with_config(false, vec!["phi".into()], vec![]);
        let mut i = input();
        i.data_classes = vec!["phi".into()];
        assert_eq!(c.classify(&i).locus, Locus::Endpoint);

        i.data_classes = vec!["secret".into()];
        assert_eq!(c.classify(&i).locus, Locus::Endpoint); // default input otherwise
        i.data_classes = vec!["internal".into()];
        i.estimated_complexity = Complexity::High;
        assert_eq!(c.classify(&i).locus, Locus::Hosted);
    }

    #[test]
    fn endpoint_only_tools_force_endpoint() {
        let c = RulesClassifier::with_config(
            false,
            vec![],
            vec!["cua.click".into(), "cua.type".into()],
        );
        let mut i = input();
        i.required_tools = vec!["cua.click".into(), "cua.type".into()];
        i.estimated_horizon = Horizon::LongHorizon;
        i.estimated_complexity = Complexity::High;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 0.9);
    }

    #[test]
    fn mixed_tools_do_not_trigger_endpoint_only_rule() {
        let c = RulesClassifier::with_config(false, vec![], vec!["cua.click".into()]);
        let mut i = input();
        i.required_tools = vec!["cua.click".into(), "email.read".into()];
        i.estimated_horizon = Horizon::LongHorizon;
        assert_eq!(c.classify(&i).locus, Locus::Split);
    }

    #[test]
    fn long_horizon_with_network_goes_split() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.estimated_horizon = Horizon::LongHorizon;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Split);
        assert_eq!(d.confidence, 0.85);
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn no_local_model_goes_hosted() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.local_model_available = false;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Hosted);
        assert_eq!(d.confidence, 0.8);
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn high_complexity_goes_hosted() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.estimated_complexity = Complexity::High;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Hosted);
        assert_eq!(d.confidence, 0.7);
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn default_is_endpoint() {
        let c = RulesClassifier::new();
        let d = c.classify(&input());
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 0.6);
        assert_eq!(d.fallback, None);
    }

    #[test]
    fn hosted_and_split_decisions_carry_endpoint_fallback() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.user_preference = UserLocusPref::Background;
        assert_eq!(c.classify(&i).fallback, Some(Locus::Endpoint));

        let mut i = input();
        i.estimated_horizon = Horizon::LongHorizon;
        assert_eq!(c.classify(&i).fallback, Some(Locus::Endpoint));

        // Endpoint decisions never need a fallback.
        assert_eq!(c.classify(&input()).fallback, None);
    }
}
