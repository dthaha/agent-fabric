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

#[tokio::test]
async fn safety_check_unsafe() {
    let server = spawn_mock(
        r#"{"choices": [{"message": {"content": "unsafe"}}]}"#,
        "200 OK",
    );

    let config = SafetyConfig {
        endpoint_url: server.url(),
        model: "test-model".into(),
        parser: "mock".into(),
        timeout_ms: 5000,
        fail_mode: 0,
        rules: vec![],
        default_action: ProtoSafetyAction::Allow as i32,
        api_key: String::new(),
        extra_body_json: String::new(),
        system_prompt: String::new(),
    };

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

    let config = SafetyConfig {
        endpoint_url: server.url(),
        model: "test-model".into(),
        parser: "mock".into(),
        timeout_ms: 5000,
        fail_mode: 0,
        rules: vec![],
        default_action: ProtoSafetyAction::Allow as i32,
        api_key: String::new(),
        extra_body_json: String::new(),
        system_prompt: String::new(),
    };

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

    let config = SafetyConfig {
        endpoint_url: server.url(),
        model: "test-model".into(),
        parser: "mock".into(),
        timeout_ms: 5000,
        fail_mode: 0,
        rules: vec![],
        default_action: ProtoSafetyAction::Allow as i32,
        api_key: String::new(),
        extra_body_json: String::new(),
        system_prompt: String::new(),
    };

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

    let config = SafetyConfig {
        endpoint_url: server.url(),
        model: "test-model".into(),
        parser: "mock".into(),
        timeout_ms: 5000,
        fail_mode: 1,
        rules: vec![],
        default_action: ProtoSafetyAction::Allow as i32,
        api_key: String::new(),
        extra_body_json: String::new(),
        system_prompt: String::new(),
    };

    let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
    let result = client.check("test").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn fail_mode_open_allows_on_http_error() {
    let server = spawn_mock("not json", "200 OK");

    let config = SafetyConfig {
        endpoint_url: server.url(),
        model: "test-model".into(),
        parser: "mock".into(),
        timeout_ms: 5000,
        fail_mode: 2,
        rules: vec![],
        default_action: ProtoSafetyAction::Allow as i32,
        api_key: String::new(),
        extra_body_json: String::new(),
        system_prompt: String::new(),
    };

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

    let config = SafetyConfig {
        endpoint_url: server.url(),
        model: "test-model".into(),
        parser: "mock".into(),
        timeout_ms: 5000,
        fail_mode: 0,
        rules: vec![SafetyPolicyRule {
            category: "violence".into(),
            action: ProtoSafetyAction::Block as i32,
        }],
        default_action: ProtoSafetyAction::Allow as i32,
        api_key: String::new(),
        extra_body_json: String::new(),
        system_prompt: String::new(),
    };

    let client = SafetyClient::with_parser(config, Box::new(MockSafetyParser));
    let verdict = client.check("violent content").await.unwrap();

    assert_eq!(verdict.verdict, SafetyLevel::Unsafe);
    assert!(!verdict.categories.is_empty());
    assert!(verdict.categories.contains(&SafetyCategory::Violence));
}
