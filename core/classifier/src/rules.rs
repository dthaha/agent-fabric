//! Deterministic rules engine — the decision layer. Classification runs in
//! three phases:
//!
//! 1. **Hard constraints** (kill switch, user prefs, no network,
//!    endpoint-only tools, restricted data) always fire first. No model
//!    advisory can override them.
//! 2. **Model advisory**: when a [`ModelAdvisory`](crate::ModelAdvisory) is
//!    present, its suggested locus is used as the semantic estimate. The
//!    model's detected data classes are merged into the input beforehand —
//!    additive only, never subtractive.
//! 3. **Heuristic fallbacks** (long horizon → split, no local model →
//!    hosted, high complexity → hosted): less accurate than a model, kept
//!    for cold start (no model seeded yet) and as a safety net.

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
        // Pre-processing: merge model-detected data classes into the input.
        // The model is a sensor — it can flag data the caller missed.
        // Deny wins: model flags are additive, never subtractive.
        let effective_input = if let Some(advisory) = &input.model_advisory {
            if advisory.detected_data_classes.is_empty() {
                std::borrow::Cow::Borrowed(input)
            } else {
                let mut merged = input.clone();
                for class in &advisory.detected_data_classes {
                    if !merged.data_classes.contains(class) {
                        merged.data_classes.push(class.clone());
                    }
                }
                std::borrow::Cow::Owned(merged)
            }
        } else {
            std::borrow::Cow::Borrowed(input)
        };
        let input = effective_input.as_ref();

        // ═══ PHASE 1: HARD CONSTRAINTS ═══
        // These always fire. No model advisory can override them.
        // Rules: kill switch, user prefs (background/hosted/local),
        // no network, endpoint-only tools, restricted data.

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

        // ═══ PHASE 2: SEMANTIC ESTIMATE ═══
        // If a model advisory is present, use its suggested locus.
        // The model's detected_data_classes are merged into the input's
        // data_classes BEFORE this point (done in a pre-processing step).
        // The suggestion is advisory — it was already validated against
        // hard constraints above.
        if let Some(advisory) = &input.model_advisory {
            let reason = format!(
                "model advisory: {} (confidence {:.0}%)",
                locus_name(advisory.suggested_locus),
                advisory.confidence * 100.0
            );
            let fallback = if advisory.suggested_locus != Locus::Endpoint {
                Some(Locus::Endpoint)
            } else {
                None
            };
            return decision(
                advisory.suggested_locus,
                reason,
                advisory.confidence,
                fallback,
            );
        }

        // ═══ PHASE 3: HEURISTIC FALLBACKS ═══
        // No model advisory available. Fall back to deterministic heuristics.
        // These are LESS ACCURATE than a model — they exist for the cold-start
        // case (no model seeded yet) and as a safety net if the model fails.

        // 8. Heuristic fallback: long-horizon work favours a hosted brain
        //    with endpoint hands via the bridge.
        if input.estimated_horizon == Horizon::LongHorizon && input.network_available {
            return decision(
                Locus::Split,
                "long-horizon task — hosted inference, endpoint tools",
                0.85,
                Some(Locus::Endpoint),
            );
        }
        // 9. Heuristic fallback: nothing to think with locally.
        if !input.local_model_available && input.network_available {
            return decision(
                Locus::Hosted,
                "no local model available",
                0.8,
                Some(Locus::Endpoint),
            );
        }
        // 10. Heuristic fallback: heavy reasoning favours hosted inference.
        if input.estimated_complexity == Complexity::High && input.network_available {
            return decision(
                Locus::Hosted,
                "high complexity — hosted inference",
                0.7,
                Some(Locus::Endpoint),
            );
        }

        // ═══ DEFAULT ═══
        // Think where the tools live.
        decision(Locus::Endpoint, "default to endpoint", 0.6, None)
    }
}

fn locus_name(l: Locus) -> &'static str {
    match l {
        Locus::Endpoint => "endpoint",
        Locus::Hosted => "hosted",
        Locus::Split => "split",
        _ => "unspecified",
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
            model_advisory: None,
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

    fn advisory(locus: Locus, confidence: f32) -> Option<crate::ModelAdvisory> {
        Some(crate::ModelAdvisory {
            suggested_locus: locus,
            complexity: Complexity::Medium,
            horizon: Horizon::MultiTurn,
            detected_data_classes: vec![],
            confidence,
        })
    }

    #[test]
    fn model_advisory_hosted_with_network() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.model_advisory = advisory(Locus::Hosted, 0.9);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Hosted);
        assert!(d.reason.contains("model advisory: hosted"));
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn model_advisory_split_with_network() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.model_advisory = advisory(Locus::Split, 0.8);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Split);
        assert!(d.reason.contains("model advisory: split"));
        assert_eq!(d.fallback, Some(Locus::Endpoint));
    }

    #[test]
    fn model_advisory_endpoint() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.model_advisory = advisory(Locus::Endpoint, 0.7);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.fallback, None);
    }

    #[test]
    fn model_advisory_overridden_by_kill_switch() {
        let c = RulesClassifier::with_config(true, vec![], vec![]);
        let mut i = input();
        i.model_advisory = advisory(Locus::Hosted, 0.99);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn model_advisory_overridden_by_no_network() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.network_available = false;
        i.model_advisory = advisory(Locus::Hosted, 0.99);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn model_advisory_overridden_by_restricted_data() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.data_classes = vec!["secret".into()];
        i.model_advisory = advisory(Locus::Hosted, 0.99);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert!(d.reason.contains("secret"));
    }

    #[test]
    fn model_advisory_overridden_by_user_pref_local() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.user_preference = UserLocusPref::PreferLocal;
        i.model_advisory = advisory(Locus::Hosted, 0.99);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert_eq!(d.confidence, 0.95);
    }

    #[test]
    fn model_detected_data_classes_merged() {
        let c = RulesClassifier::new();
        let mut i = input();
        let mut adv = advisory(Locus::Hosted, 0.9).unwrap();
        adv.detected_data_classes = vec!["pii".into()];
        i.model_advisory = Some(adv);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert!(d.reason.contains("pii"));
    }

    #[test]
    fn model_detected_data_classes_additive_not_subtractive() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.data_classes = vec!["secret".into()];
        i.model_advisory = advisory(Locus::Hosted, 0.9);
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Endpoint);
        assert!(d.reason.contains("secret"));
    }

    #[test]
    fn heuristic_fallback_when_no_advisory() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.estimated_complexity = Complexity::High;
        let d = c.classify(&i);
        assert_eq!(d.locus, Locus::Hosted);
        assert_eq!(d.confidence, 0.7);
    }

    #[test]
    fn advisory_confidence_propagated() {
        let c = RulesClassifier::new();
        let mut i = input();
        i.model_advisory = advisory(Locus::Hosted, 0.85);
        let d = c.classify(&i);
        assert_eq!(d.confidence, 0.85);
    }
}
