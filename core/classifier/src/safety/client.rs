#[cfg(feature = "safety-granite-guardian")]
use crate::safety::granite_guardian::GraniteGuardianParser;
#[cfg(feature = "safety-llama-guard")]
use crate::safety::llama_guard::LlamaGuardParser;
#[cfg(feature = "safety-nemotron-cs")]
use crate::safety::nemotron_cs::NemotronContentSafetyParser;
#[cfg(feature = "safety-shield-gemma")]
use crate::safety::shield_gemma::ShieldGemmaParser;
use crate::safety::{SafetyError, SafetyLevel, SafetyParser, SafetyVerdict};

use fabric_types::policy::{FailMode, SafetyConfig};

pub fn parser_from_name(name: &str) -> Result<Box<dyn SafetyParser>, String> {
    match name {
        #[cfg(feature = "safety-granite-guardian")]
        "granite_guardian" => Ok(Box::new(GraniteGuardianParser::new())),
        #[cfg(feature = "safety-llama-guard")]
        "llama_guard" => Ok(Box::new(LlamaGuardParser::new())),
        #[cfg(feature = "safety-nemotron-cs")]
        "nemotron_cs" => Ok(Box::new(NemotronContentSafetyParser::new())),
        #[cfg(feature = "safety-shield-gemma")]
        "shield_gemma" => Ok(Box::new(ShieldGemmaParser::new())),
        other => Err(format!("unknown safety parser: '{other}'")),
    }
}

/// Client-level default timeout. Covers the entire request including body
/// reads; the per-request `tokio::time::timeout` on `config.timeout_ms` is
/// an additional, tighter cap on the send.
const DEFAULT_CLIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Hard cap on a response body (1 MiB). A safety verdict is a handful of
/// bytes; anything larger is a misbehaving or hostile endpoint and must not
/// be buffered unboundedly.
const MAX_RESPONSE_BYTES: usize = 1_048_576;

fn default_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DEFAULT_CLIENT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub struct SafetyClient {
    http: reqwest::Client,
    config: SafetyConfig,
    parser: Box<dyn SafetyParser>,
}

impl SafetyClient {
    pub fn new(config: SafetyConfig) -> Result<Self, String> {
        let parser = parser_from_name(&config.parser)?;
        Ok(Self {
            http: default_client(),
            config,
            parser,
        })
    }

    pub fn with_parser(config: SafetyConfig, parser: Box<dyn SafetyParser>) -> Self {
        Self {
            http: default_client(),
            config,
            parser,
        }
    }

    pub fn parser(&self) -> &dyn SafetyParser {
        self.parser.as_ref()
    }

    pub fn config(&self) -> &SafetyConfig {
        &self.config
    }

    /// The chat-completions URL. `endpoint_url` may be the base
    /// (`http://host:port`), the API root (`http://host:port/v1`), or the
    /// full path (`http://host:port/v1/chat/completions`) — all three
    /// conventions resolve to the same URL.
    fn completions_url(&self) -> String {
        let base = self.config.endpoint_url.trim_end_matches('/');
        if base.ends_with("/v1/chat/completions") {
            base.to_string()
        } else if let Some(root) = base.strip_suffix("/v1") {
            format!("{root}/v1/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    /// The system prompt for this check: the policy-pack override, else the
    /// parser's own default, else no system message at all.
    fn system_prompt(&self) -> &str {
        if !self.config.system_prompt.trim().is_empty() {
            return self.config.system_prompt.as_str();
        }
        self.parser.default_system_prompt()
    }

    /// Build the chat-completions request body: standard OpenAI fields plus
    /// any vendor extensions from `extra_body_json` (which cannot override
    /// standard fields — same convention as `ConstrainedDecoder`).
    fn request_body(&self, content: &str) -> Result<serde_json::Value, SafetyError> {
        let system = self.system_prompt();
        let mut messages = Vec::new();
        if !system.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": content}));

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": 256,
            "temperature": 0.0
        });

        let extra_raw = self.config.extra_body_json.trim();
        if !extra_raw.is_empty() {
            let extra: serde_json::Map<String, serde_json::Value> = serde_json::from_str(extra_raw)
                .map_err(|e| SafetyError::Http(format!("invalid extra_body_json: {e}")))?;
            for (k, v) in extra {
                if !body.as_object().unwrap().contains_key(&k) {
                    body[k] = v;
                }
            }
        }
        Ok(body)
    }

    pub async fn check(&self, content: &str) -> Result<SafetyVerdict, SafetyError> {
        match self.check_inner(content).await {
            Ok(verdict) => Ok(verdict),
            Err(e) if self.config.fail_mode == FailMode::Open as i32 => {
                // Fail-open: a safety-endpoint outage must not block work.
                Ok(SafetyVerdict {
                    verdict: SafetyLevel::Safe,
                    categories: Vec::new(),
                    explanation: Some(format!("fail-open after error: {e}")),
                    model_id: self.config.model.clone(),
                    raw_output: String::new(),
                })
            }
            Err(e) => Err(e),
        }
    }

    async fn check_inner(&self, content: &str) -> Result<SafetyVerdict, SafetyError> {
        let body = self.request_body(content)?;

        let timeout = tokio::time::Duration::from_millis(self.config.timeout_ms as u64);
        let url = self.completions_url();
        let api_key = &self.config.api_key;

        let response = tokio::time::timeout(timeout, async {
            let mut req = self.http.post(&url).json(&body);
            if !api_key.trim().is_empty() {
                req = req.bearer_auth(api_key);
            }
            req.send().await
        })
        .await
        .map_err(|_| SafetyError::Timeout(self.config.timeout_ms as u64))?;

        let response = response.map_err(SafetyError::from)?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(SafetyError::Http(format!(
                "endpoint returned {status}: {body_text}"
            )));
        }

        let response_json: serde_json::Value = {
            let bytes = response.bytes().await.map_err(SafetyError::from)?;
            if bytes.len() > MAX_RESPONSE_BYTES {
                return Err(SafetyError::Http(format!(
                    "response body exceeded {MAX_RESPONSE_BYTES} bytes"
                )));
            }
            serde_json::from_slice(&bytes)
                .map_err(|e| SafetyError::Http(format!("failed to parse response JSON: {e}")))?
        };

        let raw_output = response_json
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SafetyError::Http("response missing choices[0].message.content".into())
            })?;

        let verdict = self.parser.parse(raw_output, &self.config.model)?;

        Ok(verdict)
    }
}

/// Debug view of the generated `SafetyConfig` with the bearer token
/// redacted. The prost-generated `Debug` would print `api_key` in the clear.
struct RedactedSafetyConfig<'a>(&'a SafetyConfig);

impl std::fmt::Debug for RedactedSafetyConfig<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c = self.0;
        f.debug_struct("SafetyConfig")
            .field("endpoint_url", &c.endpoint_url)
            .field("model", &c.model)
            .field("parser", &c.parser)
            .field("timeout_ms", &c.timeout_ms)
            .field("fail_mode", &c.fail_mode)
            .field("rules", &c.rules)
            .field("default_action", &c.default_action)
            .field("api_key", &"[REDACTED]")
            .field("extra_body_json", &c.extra_body_json)
            .field("system_prompt", &c.system_prompt)
            .finish()
    }
}

impl std::fmt::Debug for SafetyClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafetyClient")
            .field("config", &RedactedSafetyConfig(&self.config))
            .field("parser", &self.parser.name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::{ParseError, SafetyCategory};
    use fabric_types::policy::SafetyAction;

    fn test_config(endpoint_url: &str) -> SafetyConfig {
        SafetyConfig {
            endpoint_url: endpoint_url.into(),
            model: "test-model".into(),
            parser: "nemotron_cs".into(),
            timeout_ms: 500,
            fail_mode: FailMode::Closed as i32,
            rules: vec![],
            default_action: SafetyAction::Block as i32,
            api_key: String::new(),
            extra_body_json: String::new(),
            system_prompt: String::new(),
        }
    }

    struct StubParser;

    impl SafetyParser for StubParser {
        fn parse(&self, raw_output: &str, model_id: &str) -> Result<SafetyVerdict, ParseError> {
            Ok(SafetyVerdict {
                verdict: SafetyLevel::Safe,
                categories: Vec::<SafetyCategory>::new(),
                explanation: None,
                model_id: model_id.to_string(),
                raw_output: raw_output.to_string(),
            })
        }
        fn name(&self) -> &str {
            "stub"
        }
    }

    /// A URL nothing is listening on (bind, grab the port, drop).
    async fn dead_url() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}")
    }

    #[test]
    fn parser_from_name_known() {
        #[cfg(feature = "safety-granite-guardian")]
        assert!(parser_from_name("granite_guardian").is_ok());
        #[cfg(feature = "safety-llama-guard")]
        assert!(parser_from_name("llama_guard").is_ok());
        #[cfg(feature = "safety-nemotron-cs")]
        assert!(parser_from_name("nemotron_cs").is_ok());
        #[cfg(feature = "safety-shield-gemma")]
        assert!(parser_from_name("shield_gemma").is_ok());
    }

    #[test]
    fn parser_from_name_unknown() {
        assert!(parser_from_name("nonexistent").is_err());
    }

    #[cfg(feature = "safety-granite-guardian")]
    #[test]
    fn safety_config_defaults() {
        let config = SafetyConfig {
            endpoint_url: "http://localhost:8080".into(),
            model: "granite-guardian-3.0".into(),
            parser: "granite_guardian".into(),
            timeout_ms: 5000,
            fail_mode: 0,
            rules: vec![],
            default_action: SafetyAction::Block as i32,
            api_key: String::new(),
            extra_body_json: String::new(),
            system_prompt: String::new(),
        };
        let client = SafetyClient::new(config).unwrap();
        assert_eq!(client.parser().name(), "granite_guardian");
    }

    #[test]
    fn completions_url_accepts_base_v1_and_full_path() {
        let client = |url: &str| SafetyClient::with_parser(test_config(url), Box::new(StubParser));
        assert_eq!(
            client("http://host:8080").completions_url(),
            "http://host:8080/v1/chat/completions"
        );
        assert_eq!(
            client("http://host:8080/").completions_url(),
            "http://host:8080/v1/chat/completions"
        );
        assert_eq!(
            client("http://host:8080/v1").completions_url(),
            "http://host:8080/v1/chat/completions"
        );
        assert_eq!(
            client("http://host:8080/v1/chat/completions").completions_url(),
            "http://host:8080/v1/chat/completions"
        );
    }

    #[test]
    fn request_body_omits_system_message_when_no_prompt() {
        let client = SafetyClient::with_parser(test_config("http://x"), Box::new(StubParser));
        let body = client.request_body("hi").unwrap();
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn request_body_uses_config_system_prompt_override() {
        let mut cfg = test_config("http://x");
        cfg.system_prompt = "custom prompt".into();
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        let body = client.request_body("hi").unwrap();
        assert_eq!(body["messages"][0]["content"], "custom prompt");
    }

    #[cfg(feature = "safety-nemotron-cs")]
    #[test]
    fn request_body_falls_back_to_parser_default_prompt() {
        let client = SafetyClient::new(test_config("http://x")).unwrap();
        let body = client.request_body("hi").unwrap();
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("User Safety:"));
    }

    #[test]
    fn extra_body_merges_but_cannot_override_standard_fields() {
        let mut cfg = test_config("http://x");
        cfg.extra_body_json = r#"{"reasoning": {"effort": "none"}, "temperature": 9.9}"#.into();
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        let body = client.request_body("hi").unwrap();
        assert_eq!(body["reasoning"]["effort"], "none");
        assert_eq!(body["temperature"], 0.0, "standard fields win");
    }

    #[test]
    fn invalid_extra_body_json_is_an_error() {
        let mut cfg = test_config("http://x");
        cfg.extra_body_json = "{not json".into();
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        assert!(client.request_body("hi").is_err());
    }

    #[test]
    fn debug_redacts_api_key() {
        let mut cfg = test_config("http://x");
        cfg.api_key = "super-secret-token".into();
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        let debug = format!("{client:?}");
        assert!(!debug.contains("super-secret-token"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }

    #[tokio::test]
    async fn fail_closed_propagates_http_errors() {
        let mut cfg = test_config(&dead_url().await);
        cfg.fail_mode = FailMode::Closed as i32;
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        assert!(client.check("hi").await.is_err());
    }

    #[tokio::test]
    async fn fail_open_returns_safe_on_http_errors() {
        let mut cfg = test_config(&dead_url().await);
        cfg.fail_mode = FailMode::Open as i32;
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        let verdict = client.check("hi").await.unwrap();
        assert_eq!(verdict.verdict, SafetyLevel::Safe);
        assert!(verdict.categories.is_empty());
        assert!(verdict.explanation.unwrap().contains("fail-open"));
    }

    #[tokio::test]
    async fn fail_open_returns_safe_on_config_errors() {
        let mut cfg = test_config("http://x");
        cfg.fail_mode = FailMode::Open as i32;
        cfg.extra_body_json = "{not json".into();
        let client = SafetyClient::with_parser(cfg, Box::new(StubParser));
        let verdict = client.check("hi").await.unwrap();
        assert_eq!(verdict.verdict, SafetyLevel::Safe);
    }
}
