#[cfg(feature = "safety-granite-guardian")]
use crate::safety::granite_guardian::GraniteGuardianParser;
#[cfg(feature = "safety-llama-guard")]
use crate::safety::llama_guard::LlamaGuardParser;
#[cfg(feature = "safety-nemotron-cs")]
use crate::safety::nemotron_cs::NemotronContentSafetyParser;
#[cfg(feature = "safety-shield-gemma")]
use crate::safety::shield_gemma::ShieldGemmaParser;
use crate::safety::{SafetyError, SafetyParser, SafetyVerdict};

use fabric_types::policy::SafetyConfig;

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

pub struct SafetyClient {
    http: reqwest::Client,
    config: SafetyConfig,
    parser: Box<dyn SafetyParser>,
}

impl SafetyClient {
    pub fn new(config: SafetyConfig) -> Result<Self, String> {
        let parser = parser_from_name(&config.parser)?;
        Ok(Self {
            http: reqwest::Client::new(),
            config,
            parser,
        })
    }

    pub fn with_parser(config: SafetyConfig, parser: Box<dyn SafetyParser>) -> Self {
        Self {
            http: reqwest::Client::new(),
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

    pub async fn check(&self, content: &str) -> Result<SafetyVerdict, SafetyError> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a safety content classifier. Analyze the user's input and determine if it contains unsafe content. Respond with the safety verdict and any detected risk categories."
                },
                {
                    "role": "user",
                    "content": content
                }
            ],
            "max_tokens": 256,
            "temperature": 0.0
        });

        let timeout = tokio::time::Duration::from_millis(self.config.timeout_ms as u64);

        let response = tokio::time::timeout(timeout, async {
            self.http
                .post(format!(
                    "{}/v1/chat/completions",
                    self.config.endpoint_url.trim_end_matches('/')
                ))
                .json(&body)
                .send()
                .await
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

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SafetyError::Http(format!("failed to parse response JSON: {e}")))?;

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

impl std::fmt::Debug for SafetyClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafetyClient")
            .field("config", &self.config)
            .field("parser", &self.parser.name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::policy::SafetyAction;

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
        };
        let client = SafetyClient::new(config).unwrap();
        assert_eq!(client.parser().name(), "granite_guardian");
    }
}
