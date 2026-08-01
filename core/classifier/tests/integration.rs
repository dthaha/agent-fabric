//! Full classification pipeline: rules engine wrapped in the policy gate,
//! plus the safety client/enforcer integration tests.
//!
//! Exercises the downgrade paths that keep the classifier honest when policy
//! forbids server execution, and the safety pipeline (client → parser →
//! enforcer) with a mock HTTP server.

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

    // Deny-wins: restricted data beats an explicit server preference. The
    // rules engine itself pins the turn to the endpoint before any
    // user-preference rule can fire.
    let classifier =
        PolicyAwareClassifier::new(RulesClassifier::new(), gate(vec![inference_rule()], vec![]));
    let mut input = server_input();
    input.data_classes = vec!["secret".into()];
    let d = classifier.classify(&input);
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("secret"));
    assert!(!d.reason.contains("downgraded"));
}

#[test]
fn require_approval_egress_blocks_server_like_deny() {
    // requires_redaction maps to RequireApproval in the gate; on the
    // synchronous classify path that blocks egress just like a Deny.
    let data_rules = vec![DataClassRule {
        data_class: "internal".into(),
        may_leave_device: true,
        requires_redaction: true,
        allowed_destinations: vec!["server".into()],
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
fn kill_switch_downgrades_server_decisions() {
    let g = PolicyGate::new(policy(vec![inference_rule()], vec![], true));
    let classifier = PolicyAwareClassifier::new(RulesClassifier::new(), g);
    let d = classifier.classify(&server_input());
    assert_eq!(d.locus, Locus::Endpoint);
    assert!(d.reason.contains("kill switch"));
}

// ---------------------------------------------------------------------------
// Safety client / enforcer integration (mock HTTP server, no real endpoint)
// ---------------------------------------------------------------------------

mod safety {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use fabric_classifier::safety::client::SafetyClient;
    use fabric_classifier::safety::{SafetyAction, SafetyCategory, SafetyLevel, SafetyVerdict};
    use fabric_classifier::safety::{SafetyParser, SafetyPolicyEnforcer};
    use fabric_types::policy::{SafetyAction as ProtoSafetyAction, SafetyConfig, SafetyPolicyRule};

    struct MockSafetyParser;

    impl SafetyParser for MockSafetyParser {
        fn parse(
            &self,
            raw_output: &str,
            model_id: &str,
        ) -> Result<SafetyVerdict, fabric_classifier::safety::ParseError> {
            if raw_output.contains("unsafe") || raw_output.contains("violence") {
                Ok(SafetyVerdict {
                    verdict: SafetyLevel::Unsafe,
                    categories: vec![SafetyCategory::Violence],
                    explanation: None,
                    model_id: model_id.into(),
                    raw_output: raw_output.into(),
                })
            } else {
                Ok(SafetyVerdict {
                    verdict: SafetyLevel::Safe,
                    categories: vec![],
                    explanation: None,
                    model_id: model_id.into(),
                    raw_output: raw_output.into(),
                })
            }
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    struct MockServerGuard {
        port: u16,
        shutdown: Arc<AtomicBool>,
    }

    impl MockServerGuard {
        fn url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }
    }

    impl Drop for MockServerGuard {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
        }
    }

    fn spawn_mock(response_body: &'static str, status_code: &'static str) -> MockServerGuard {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = std::net::TcpListener::bind(addr).unwrap();
        let port = listener.local_addr().unwrap().port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_server = Arc::clone(&shutdown);

        std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut reader = BufReader::new(&stream);
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                                break;
                            }
                            if line.trim().is_empty() {
                                break;
                            }
                        }
                        drop(reader);

                        let response = format!(
                            "HTTP/1.1 {status_code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                            response_body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if shutdown_server.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        // Wait for the server to be ready
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        MockServerGuard { port, shutdown }
    }

    fn safety_config(url: String, fail_mode: i32, rules: Vec<SafetyPolicyRule>) -> SafetyConfig {
        SafetyConfig {
            endpoint_url: url,
            model: "test-model".into(),
            parser: "mock".into(),
            timeout_ms: 5000,
            fail_mode,
            rules,
            default_action: ProtoSafetyAction::Allow as i32,
            api_key: String::new(),
            extra_body_json: String::new(),
            system_prompt: String::new(),
        }
    }

    #[tokio::test]
    async fn safety_check_unsafe() {
        let server = spawn_mock(
            r#"{"choices": [{"message": {"content": "unsafe"}}]}"#,
            "200 OK",
        );
        let config = safety_config(server.url(), 0, vec![]);
        let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
        let verdict = client.check("some harmful content").await.unwrap();
        assert_eq!(verdict.verdict, SafetyLevel::Unsafe);
    }

    #[tokio::test]
    async fn safety_check_safe() {
        let server = spawn_mock(
            r#"{"choices": [{"message": {"content": "safe"}}]}"#,
            "200 OK",
        );
        let config = safety_config(server.url(), 0, vec![]);
        let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
        let verdict = client.check("harmless content").await.unwrap();
        assert_eq!(verdict.verdict, SafetyLevel::Safe);
    }

    #[tokio::test]
    async fn safety_policy_blocks_unsafe() {
        let server = spawn_mock(
            r#"{"choices": [{"message": {"content": "violence detected"}}]}"#,
            "200 OK",
        );
        let config = safety_config(server.url(), 0, vec![]);
        let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
        let verdict = client.check("harmful content").await.unwrap();

        let enforcer = SafetyPolicyEnforcer::new(
            vec![SafetyPolicyRule {
                category: "violence".into(),
                action: ProtoSafetyAction::Block as i32,
            }],
            SafetyAction::Allow,
        );

        let enforcement = enforcer.enforce(&verdict);
        assert!(enforcement.blocked);
    }

    #[tokio::test]
    async fn fail_mode_closed_blocks_on_http_error() {
        let server = spawn_mock("Internal Server Error", "500 Internal Server Error");
        let config = safety_config(server.url(), 1, vec![]);
        let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
        let result = client.check("test").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fail_mode_open_allows_on_http_error() {
        let server = spawn_mock("not json", "200 OK");
        let config = safety_config(server.url(), 2, vec![]);
        let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
        let verdict = client.check("test").await.unwrap();
        assert_eq!(verdict.verdict, SafetyLevel::Safe);
    }

    #[tokio::test]
    async fn full_pipeline_safety_client_policy() {
        let server = spawn_mock(
            r#"{"choices": [{"message": {"content": "violence in content"}}]}"#,
            "200 OK",
        );
        let config = safety_config(
            server.url(),
            0,
            vec![SafetyPolicyRule {
                category: "violence".into(),
                action: ProtoSafetyAction::Block as i32,
            }],
        );
        let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
        let verdict = client.check("violent content").await.unwrap();

        assert_eq!(verdict.verdict, SafetyLevel::Unsafe);
        assert!(!verdict.categories.is_empty());
        assert!(verdict.categories.contains(&SafetyCategory::Violence));
    }
}
