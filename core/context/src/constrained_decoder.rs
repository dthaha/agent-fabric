//! Reference [`ConflictDecoder`] backed by an OpenAI-compatible chat
//! completions endpoint with JSON-schema-constrained decoding.
//!
//! Endpoint-agnostic: base URL, API key, and model come from config/env
//! (`OPENAI_BASE_URL`, `OPENAI_API_KEY`, `FABRIC_DECODER_MODEL`) — never
//! hardcoded to a provider. The request asks for `response_format:
//! json_schema` (OpenAI structured-output shape) pointed at the locked
//! [`verdict_json_schema`]; endpoints that reject `response_format` fall back
//! to the plain prompt + the tolerant [`parse_verdict`].
//!
//! The raw completion is ALWAYS piped through [`parse_verdict`] — parsing is
//! not re-implemented here, and identity fields are still injected from the
//! [`DecoderInput`], never trusted from model output. Classify-ONLY: this
//! decoder never acts, never mutates state, never calls policy.

use std::time::Duration;

use async_trait::async_trait;
use fabric_types::conflict::ConflictVerdict;

use crate::decoder::{parse_verdict, ConflictDecoder, DecoderError, DecoderInput};

/// The reference system prompt, embedded at build time from the versioned
/// artifact. `models/conflict-decoder/system_prompt.md` is the source of
/// truth; this `include_str!` keeps the shipped binary in lock-step with it.
pub const SYSTEM_PROMPT: &str = include_str!("../../../models/conflict-decoder/system_prompt.md");

/// Default request timeout when the endpoint stalls.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Default model when `FABRIC_DECODER_MODEL` is not set.
const DEFAULT_MODEL: &str = "nvidia/nemotron-3-nano-30b-a3b";

/// Default generation parameters, tuned for classify-ONLY decoding: near-
/// deterministic sampling, reasoning OFF, short output budget. The decoder
/// is a classifier, not a reasoner: any chain-of-thought effort (even
/// `low`) burns tokens on CoT and mangles the structured JSON output, so
/// it MUST run with `reasoning: none` (eval-verified, July 2026).
const DEFAULT_TEMPERATURE: f64 = 0.1;
const DEFAULT_TOP_K: u32 = 20;
const DEFAULT_TOP_P: f64 = 0.9;
const DEFAULT_REASONING_EFFORT: &str = "none";
const DEFAULT_MAX_TOKENS: u32 = 300;

/// The locked verdict output contract as a real JSON Schema, used for
/// `response_format: json_schema` constrained decoding. Mirrors
/// [`crate::decoder::OUTPUT_SCHEMA`] — same fields, same relation enum,
/// nothing else permitted.
pub fn verdict_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "relation": {
                "type": "string",
                "enum": ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"]
            },
            "shared_entities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "entity_type": {"type": "string"},
                        "entity_id": {"type": "string"}
                    },
                    "required": ["entity_type", "entity_id"],
                    "additionalProperties": false
                }
            },
            "confidence": {"type": "number", "minimum": 0, "maximum": 1},
            "explanation": {"type": "string"}
        },
        "required": ["relation", "shared_entities", "confidence", "explanation"],
        "additionalProperties": false
    })
}

/// Configuration for [`ConstrainedDecoder`]. No provider is hardcoded: any
/// OpenAI-compatible chat completions endpoint works (vLLM, Ollama, llama.cpp
/// server, a hosted provider).
#[derive(Debug, Clone, PartialEq)]
pub struct ConstrainedDecoderConfig {
    /// Base URL of the endpoint. Both conventions are accepted:
    /// `http://host:port` and `http://host:port/v1`.
    pub base_url: String,
    /// Bearer token, if the endpoint requires one. Local servers often don't.
    pub api_key: Option<String>,
    /// Model name to request (e.g. `nvidia/nemotron-3-nano-30b-a3b`).
    pub model: String,
    pub timeout_ms: u64,
    pub temperature: f64,
    pub top_k: u32,
    pub top_p: f64,
    /// OpenRouter reasoning effort (`none`, `low`, `medium`, `high`). Sent as
    /// `reasoning: {"effort": ...}` in the request body. OpenRouter does NOT
    /// honor `enable_thinking: bool`; reasoning effort is the only working
    /// control. Decoder default is `none` — CoT poisons structured output.
    pub reasoning_effort: String,
    pub max_tokens: u32,
}

impl ConstrainedDecoderConfig {
    /// Resolve config from the environment: `OPENAI_BASE_URL` (required),
    /// `OPENAI_API_KEY` (optional), `FABRIC_DECODER_MODEL` (optional; falls
    /// back to the default), `FABRIC_DECODER_TIMEOUT_MS`,
    /// `FABRIC_DECODER_TEMPERATURE`, `FABRIC_DECODER_TOP_K`,
    /// `FABRIC_DECODER_TOP_P`, `FABRIC_DECODER_REASONING_EFFORT`,
    /// `FABRIC_DECODER_MAX_TOKENS` (all optional).
    pub fn from_env() -> Result<Self, DecoderError> {
        Self::resolve(
            std::env::var("OPENAI_BASE_URL").ok(),
            std::env::var("OPENAI_API_KEY").ok(),
            std::env::var("FABRIC_DECODER_MODEL").ok(),
            std::env::var("FABRIC_DECODER_TIMEOUT_MS").ok(),
            std::env::var("FABRIC_DECODER_TEMPERATURE").ok(),
            std::env::var("FABRIC_DECODER_TOP_K").ok(),
            std::env::var("FABRIC_DECODER_TOP_P").ok(),
            std::env::var("FABRIC_DECODER_REASONING_EFFORT").ok(),
            std::env::var("FABRIC_DECODER_MAX_TOKENS").ok(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve(
        base_url: Option<String>,
        api_key: Option<String>,
        model: Option<String>,
        timeout_ms: Option<String>,
        temperature: Option<String>,
        top_k: Option<String>,
        top_p: Option<String>,
        reasoning_effort: Option<String>,
        max_tokens: Option<String>,
    ) -> Result<Self, DecoderError> {
        let base_url = base_url
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| DecoderError::Config("OPENAI_BASE_URL is not set".into()))?;
        let model = model
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let timeout_ms = match timeout_ms {
            Some(raw) => raw
                .trim()
                .parse::<u64>()
                .map_err(|_| DecoderError::Config(format!("invalid timeout '{raw}'")))?,
            None => DEFAULT_TIMEOUT_MS,
        };
        let temperature = match temperature {
            Some(raw) => raw
                .trim()
                .parse::<f64>()
                .map_err(|_| DecoderError::Config(format!("invalid temperature '{raw}'")))?,
            None => DEFAULT_TEMPERATURE,
        };
        let top_k = match top_k {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| DecoderError::Config(format!("invalid top_k '{raw}'")))?,
            None => DEFAULT_TOP_K,
        };
        let top_p = match top_p {
            Some(raw) => raw
                .trim()
                .parse::<f64>()
                .map_err(|_| DecoderError::Config(format!("invalid top_p '{raw}'")))?,
            None => DEFAULT_TOP_P,
        };
        let reasoning_effort = match reasoning_effort {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(DecoderError::Config("invalid reasoning_effort ''".into()));
                }
                trimmed.to_string()
            }
            None => DEFAULT_REASONING_EFFORT.to_string(),
        };
        let max_tokens = match max_tokens {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| DecoderError::Config(format!("invalid max_tokens '{raw}'")))?,
            None => DEFAULT_MAX_TOKENS,
        };
        Ok(ConstrainedDecoderConfig {
            base_url,
            api_key: api_key.filter(|s| !s.trim().is_empty()),
            model,
            timeout_ms,
            temperature,
            top_k,
            top_p,
            reasoning_effort,
            max_tokens,
        })
    }

    /// The chat-completions URL. Tolerates a base that already ends in `/v1`
    /// (the `OPENAI_BASE_URL` convention) and one that doesn't.
    fn completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }
}

/// Reference Tier 2 decoder: small instruct model behind an OpenAI-compatible
/// endpoint, constrained to the verdict schema, parsed by [`parse_verdict`].
/// Swappable with [`crate::decoder::StubDecoder`] behind the same trait.
pub struct ConstrainedDecoder {
    http: reqwest::Client,
    config: ConstrainedDecoderConfig,
}

impl ConstrainedDecoder {
    pub fn new(config: ConstrainedDecoderConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    pub fn from_env() -> Result<Self, DecoderError> {
        Ok(Self::new(ConstrainedDecoderConfig::from_env()?))
    }

    pub fn config(&self) -> &ConstrainedDecoderConfig {
        &self.config
    }

    /// Build the chat-completions request body. When `constrained` is true the
    /// body carries `response_format: json_schema` for structured-output
    /// decoding; when false (fallback path) the system prompt alone carries
    /// the "exactly one JSON object" contract.
    fn request_body(&self, input: &DecoderInput, constrained: bool) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": input.render_prompt()}
            ],
            "temperature": self.config.temperature,
            "top_k": self.config.top_k,
            "top_p": self.config.top_p,
            "reasoning": {"effort": self.config.reasoning_effort},
            "provider": {"sort": "throughput"},
            "max_tokens": self.config.max_tokens
        });
        if constrained {
            body["response_format"] = serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "conflict_verdict",
                    "strict": true,
                    "schema": verdict_json_schema()
                }
            });
        }
        body
    }

    /// POST one chat completion and return the raw content string.
    async fn complete(&self, body: &serde_json::Value) -> Result<String, DecoderError> {
        let url = self.config.completions_url();
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let mut req = self.http.post(&url).json(body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        let response = tokio::time::timeout(timeout, req.send())
            .await
            .map_err(|_| DecoderError::Timeout(self.config.timeout_ms))?
            .map_err(|e| DecoderError::Http(format!("request failed: {e}")))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| DecoderError::Http(format!("failed to read response body: {e}")))?;
        if !status.is_success() {
            return Err(DecoderError::Http(format!(
                "endpoint returned {status}: {text}"
            )));
        }
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| DecoderError::Http(format!("response is not JSON: {e}")))?;
        json.pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| DecoderError::Http("response missing choices[0].message.content".into()))
    }

    /// True when an endpoint error looks like "response_format/json_schema not
    /// supported" — the signal to retry unconstrained.
    fn is_schema_unsupported(err: &DecoderError) -> bool {
        match err {
            DecoderError::Http(msg) => {
                let m = msg.to_lowercase();
                m.contains("response_format") || m.contains("json_schema")
            }
            _ => false,
        }
    }
}

impl std::fmt::Debug for ConstrainedDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConstrainedDecoder")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait]
impl ConflictDecoder for ConstrainedDecoder {
    async fn decode(&self, input: DecoderInput) -> Result<ConflictVerdict, DecoderError> {
        // Preferred path: JSON-schema-constrained decoding.
        let constrained = self.request_body(&input, true);
        let raw = match self.complete(&constrained).await {
            Ok(raw) => raw,
            Err(e) if Self::is_schema_unsupported(&e) => {
                // Fallback: no response_format; the system prompt's "exactly
                // one JSON object" contract plus parse_verdict's tolerance.
                let plain = self.request_body(&input, false);
                self.complete(&plain).await?
            }
            Err(e) => return Err(e),
        };
        parse_verdict(&raw, &input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_requires_base_url_and_model() {
        assert!(matches!(
            ConstrainedDecoderConfig::resolve(
                None,
                None,
                Some("m".into()),
                None,
                None,
                None,
                None,
                None,
                None
            ),
            Err(DecoderError::Config(_))
        ));
        let cfg = ConstrainedDecoderConfig::resolve(
            Some("http://x".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(cfg.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(cfg.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(cfg.top_k, DEFAULT_TOP_K);
        assert_eq!(cfg.top_p, DEFAULT_TOP_P);
        assert_eq!(cfg.reasoning_effort, DEFAULT_REASONING_EFFORT);
        assert_eq!(cfg.max_tokens, DEFAULT_MAX_TOKENS);
        let cfg = ConstrainedDecoderConfig::resolve(
            Some("http://localhost:8000".into()),
            None,
            Some("qwen".into()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(cfg.api_key, None);
    }

    #[test]
    fn resolve_parses_timeout_and_ignores_blank_key() {
        let cfg = ConstrainedDecoderConfig::resolve(
            Some("http://x".into()),
            Some("   ".into()),
            Some("m".into()),
            Some("5000".into()),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.timeout_ms, 5000);
        assert_eq!(cfg.api_key, None);
        assert!(ConstrainedDecoderConfig::resolve(
            Some("http://x".into()),
            None,
            Some("m".into()),
            Some("not-a-number".into()),
            None,
            None,
            None,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn resolve_parses_generation_params() {
        let cfg = ConstrainedDecoderConfig::resolve(
            Some("http://x".into()),
            None,
            None,
            None,
            Some("0.3".into()),
            Some("40".into()),
            Some("0.8".into()),
            Some("low".into()),
            Some("512".into()),
        )
        .unwrap();
        assert_eq!(cfg.temperature, 0.3);
        assert_eq!(cfg.top_k, 40);
        assert_eq!(cfg.top_p, 0.8);
        assert_eq!(cfg.reasoning_effort, "low");
        assert_eq!(cfg.max_tokens, 512);
        assert!(ConstrainedDecoderConfig::resolve(
            Some("http://x".into()),
            None,
            None,
            None,
            Some("hot".into()),
            None,
            None,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn completions_url_handles_both_base_conventions() {
        let bare = ConstrainedDecoderConfig {
            base_url: "http://localhost:8000/".into(),
            api_key: None,
            model: "m".into(),
            timeout_ms: 1000,
            temperature: DEFAULT_TEMPERATURE,
            top_k: DEFAULT_TOP_K,
            top_p: DEFAULT_TOP_P,
            reasoning_effort: DEFAULT_REASONING_EFFORT.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
        };
        assert_eq!(
            bare.completions_url(),
            "http://localhost:8000/v1/chat/completions"
        );
        let v1 = ConstrainedDecoderConfig {
            base_url: "https://api.example.com/v1".into(),
            ..bare
        };
        assert_eq!(
            v1.completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    fn sample_input() -> DecoderInput {
        serde_json::from_value(serde_json::json!({
            "session_id": "s1",
            "entry_id_a": "a",
            "entry_id_b": "b",
            "call_a": {"tool_name": "set_thermostat", "target": "home:living-room",
                       "params": {"temperature": "72"}, "idempotency_key": ""},
            "call_b": {"tool_name": "set_thermostat", "target": "home:living-room",
                       "params": {"temperature": "68"}, "idempotency_key": ""},
            "context": []
        }))
        .unwrap()
    }

    fn test_decoder() -> ConstrainedDecoder {
        ConstrainedDecoder::new(ConstrainedDecoderConfig {
            base_url: "http://localhost:8000".into(),
            api_key: Some("k".into()),
            model: "qwen".into(),
            timeout_ms: 1000,
            temperature: DEFAULT_TEMPERATURE,
            top_k: DEFAULT_TOP_K,
            top_p: DEFAULT_TOP_P,
            reasoning_effort: DEFAULT_REASONING_EFFORT.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
        })
    }

    #[test]
    fn constrained_body_carries_json_schema_and_prompt() {
        let body = test_decoder().request_body(&sample_input(), true);
        assert_eq!(body["model"], "qwen");
        assert_eq!(body["reasoning"]["effort"], "none");
        assert_eq!(body["provider"]["sort"], "throughput");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
        assert!(body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("set_thermostat"));
        let rf = &body["response_format"];
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["name"], "conflict_verdict");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(
            rf["json_schema"]["schema"]["properties"]["relation"]["enum"],
            serde_json::json!(["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"])
        );
    }

    #[test]
    fn fallback_body_omits_response_format() {
        let body = test_decoder().request_body(&sample_input(), false);
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn schema_unsupported_detection() {
        assert!(ConstrainedDecoder::is_schema_unsupported(
            &DecoderError::Http("endpoint returned 400: response_format is not supported".into())
        ));
        assert!(ConstrainedDecoder::is_schema_unsupported(
            &DecoderError::Http("endpoint returned 400: unknown field json_schema".into())
        ));
        assert!(!ConstrainedDecoder::is_schema_unsupported(
            &DecoderError::Http("endpoint returned 500: boom".into())
        ));
        assert!(!ConstrainedDecoder::is_schema_unsupported(
            &DecoderError::Timeout(1000)
        ));
    }

    #[test]
    fn system_prompt_mirrors_output_schema_and_contract() {
        // Byte-exact mirror of the locked OUTPUT_SCHEMA const.
        assert!(SYSTEM_PROMPT.contains(crate::decoder::OUTPUT_SCHEMA));
        // The four relations.
        for rel in ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"] {
            assert!(SYSTEM_PROMPT.contains(rel), "missing {rel}");
        }
        // The SUPERSEDES/CONTRADICTS discriminator.
        assert!(SYSTEM_PROMPT.contains("would the user be surprised if BOTH happened"));
        // Confidence calibration anchors.
        assert!(SYSTEM_PROMPT.contains("0.95"));
        assert!(SYSTEM_PROMPT.contains("0.5"));
        // The AMBIGUOUS bias.
        assert!(SYSTEM_PROMPT.contains("prefer AMBIGUOUS over a low-confidence"));
        // The anti-helpfulness firewall.
        assert!(SYSTEM_PROMPT.contains("You do NOT resolve the conflict"));
        assert!(SYSTEM_PROMPT.contains("EXACTLY one JSON object"));
        // Few-shot examples present.
        assert!(SYSTEM_PROMPT.contains("Example 5"));
    }
}
