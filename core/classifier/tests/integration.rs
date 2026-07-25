//! Full classification pipeline: rules engine wrapped in the policy gate.
//! Exercises the downgrade paths that keep the classifier honest when policy
//! forbids server execution.

use fabric_classifier::{
    ClassifyInput, Complexity, LocusClassifier, PolicyAwareClassifier, RulesClassifier,
    UserLocusPref,
};
use fabric_policy::PolicyGate;
use fabric_types::context::Locus;
use fabric_types::policy::{DataClassRule, EffectivePolicy, InferenceRule};

fn gate(inference_rules: Vec<InferenceRule>, data_rules: Vec<DataClassRule>) -> PolicyGate {
    PolicyGate::new(policy(inference_rules, data_rules, false))
}

fn policy(
    inference_rules: Vec<InferenceRule>,
    data_rules: Vec<DataClassRule>,
    kill_switch: bool,
) -> EffectivePolicy {
    EffectivePolicy {
        endpoint_version: "1".into(),
        server_version: "1".into(),
        data_rules,
        tool_rules: vec![],
        model_rules: vec![],
        cua: None,
        inference_rules,
        kill_switch,
        max_retention_hours: 0,
        background_quota: None,
        max_session_duration_hours: 0,
        max_concurrent_sessions: 0,
    }
}

fn inference_rule() -> InferenceRule {
    InferenceRule {
        provider: "bedrock".into(),
        allowed_models: vec!["claude-*".into()],
        allowed_regions: vec![],
        max_tokens_per_request: 8192,
        daily_token_budget: 0,
    }
}

fn egress_allowed_rule() -> DataClassRule {
    DataClassRule {
        data_class: "internal".into(),
        may_leave_device: true,
        requires_redaction: false,
        allowed_destinations: vec!["server".into()],
    }
}

/// Input the rules engine will always send server (explicit preference,
/// network up).
fn server_input() -> ClassifyInput {
    ClassifyInput {
        intent_text: "summarize my emails".into(),
        required_tools: vec![],
        estimated_complexity: Complexity::Low,
        estimated_horizon: fabric_classifier::Horizon::SingleTurn,
        data_classes: vec![],
        network_available: true,
        local_model_available: true,
        user_preference: UserLocusPref::PreferServer,
        model_advisory: None,
    }
}

#[test]
fn downgrades_server_when_no_inference_rules() {
    let rules = RulesClassifier::new();
    assert_eq!(rules.classify(&server_input()).locus, Locus::Server);

    let classifier = PolicyAwareClassifier::new(rules, gate(vec![], vec![]));
    let d = classifier.classify(&server_input());
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("no server-side inference rules"));
    assert_eq!(d.fallback, None);
}

#[test]
fn downgrades_server_when_data_egress_denied() {
    let data_rules = vec![DataClassRule {
        data_class: "internal".into(),
        may_leave_device: false,
        requires_redaction: false,
        allowed_destinations: vec![],
    }];
    let classifier = PolicyAwareClassifier::new(
        RulesClassifier::new(),
        gate(vec![inference_rule()], data_rules),
    );

    let mut input = server_input();
    input.data_classes = vec!["internal".into()];
    let d = classifier.classify(&input);
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("downgraded to endpoint"));
    assert!(d.reason.contains("internal"));
}

#[test]
fn passes_endpoint_decisions_through_unchanged() {
    let rules = RulesClassifier::new();
    let mut plain = server_input();
    plain.user_preference = UserLocusPref::NoPreference;
    let expected = rules.classify(&plain);
    assert_eq!(expected.locus, Locus::Endpoint);

    let classifier = PolicyAwareClassifier::new(rules, gate(vec![], vec![]));
    assert_eq!(classifier.classify(&plain), expected);
}

#[test]
fn full_pipeline_respects_rules_and_policy() {
    // Rules say split (long horizon); policy permits server-side inference and
    // the internal data class may egress — decision survives.
    let classifier = PolicyAwareClassifier::new(
        RulesClassifier::new(),
        gate(vec![inference_rule()], vec![egress_allowed_rule()]),
    );
    let mut input = server_input();
    input.user_preference = UserLocusPref::NoPreference;
    input.estimated_horizon = fabric_classifier::Horizon::LongHorizon;
    input.data_classes = vec!["internal".into()];
    let d = classifier.classify(&input);
    assert_eq!(d.locus, Locus::Split);
    assert_eq!(d.fallback, Some(Locus::Endpoint));

    // Same rules, but policy has no inference provider: downgraded.
    let classifier = PolicyAwareClassifier::new(RulesClassifier::new(), gate(vec![], vec![]));
    let d = classifier.classify(&input);
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("long-horizon"));

    // Rules pin restricted data to the endpoint before policy is consulted.
    let classifier =
        PolicyAwareClassifier::new(RulesClassifier::new(), gate(vec![inference_rule()], vec![]));
    let mut input = server_input();
    input.user_preference = UserLocusPref::NoPreference;
    input.data_classes = vec!["secret".into()];
    let d = classifier.classify(&input);
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(!d.reason.contains("downgraded"));

    // An explicit server preference outranks the restricted-data rule, but
    // the wrapper still catches it: 'secret' has no egress rule, so the
    // server decision is downgraded by policy instead.
    let classifier =
        PolicyAwareClassifier::new(RulesClassifier::new(), gate(vec![inference_rule()], vec![]));
    let mut input = server_input();
    input.data_classes = vec!["secret".into()];
    let d = classifier.classify(&input);
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("downgraded"));
}

#[test]
fn kill_switch_downgrades_server_decisions() {
    let g = PolicyGate::new(policy(vec![inference_rule()], vec![], true));
    let classifier = PolicyAwareClassifier::new(RulesClassifier::new(), g);
    let d = classifier.classify(&server_input());
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("kill switch"));
}
