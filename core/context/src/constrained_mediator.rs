//! Reference [`ConflictMediator`] backed by an OpenAI-compatible chat
//! completions endpoint with JSON-schema-constrained decoding.
//!
//! Endpoint-agnostic: base URL and API key come from the same env convention
//! as the decoder (`OPENAI_BASE_URL`, `OPENAI_API_KEY`); the model comes from
//! `FABRIC_MEDIATOR_MODEL`, falling back to `FABRIC_DECODER_MODEL` and then a
//! default — never hardcoded to a provider. The request asks for
//! `response_format: json_schema` (OpenAI structured-output shape) pointed at
//! the locked [`proposal_json_schema`]; endpoints that reject
//! `response_format` fall back to the plain prompt + the tolerant
//! [`parse_proposal`].
//!
//! The raw completion is ALWAYS piped through [`parse_proposal`] — parsing is
//! not re-implemented here, `session_id` is still injected from the
//! [`MediatorInput`], and an invented `winning_entry_id` is still cleared.
//! Propose-ONLY: this mediator never acts, never enforces, never calls
//! policy.

use std::time::Duration;

use async_trait::async_trait;
use fabric_types::conflict::ResolutionProposal;

use crate::mediator::{parse_proposal, ConflictMediator, MediatorError, MediatorInput};

/// The reference system prompt, embedded at build time from the versioned
/// artifact. `models/conflict-mediator/system_prompt.md` is the source of
/// truth; this `include_str!` keeps the shipped binary in lock-step with it.
pub const SYSTEM_PROMPT: &str = include_str!("../../../models/conflict-mediator/system_prompt.md");

/// Default request timeout when the endpoint stalls. The mediator is a
/// reasoning model on a cold path, so this is more generous than the
/// decoder's.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Default model when neither `FABRIC_MEDIATOR_MODEL` nor
/// `FABRIC_DECODER_MODEL` is set.
const DEFAULT_MODEL: &str = "nvidia/nemotron-3-nano-30b-a3b";

/// Default generation parameters, tuned for propose-ONLY mediation: deep
/// reasoning on the cold path, a modest sampling budget, room for
/// rationale. The mediator runs with `reasoning: high` — it must reason
/// deeply about resolution strategy (eval-verified, July 2026: 54.4%
/// resolution at 100% schema compliance), while the Tier 4 policy veto
/// provides the safety floor.
const DEFAULT_TEMPERATURE: f64 = 0.7;
const DEFAULT_TOP_K: u32 = 20;
const DEFAULT_TOP_P: f64 = 0.9;
const DEFAULT_REASONING_EFFORT: &str = "high";
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// The locked proposal output contract as a real JSON Schema, used for
/// `response_format: json_schema` constrained decoding. Mirrors
/// [`crate::mediator::PROPOSAL_OUTPUT_SCHEMA`] — same fields, same enums,
/// nothing else permitted. OpenAI strict mode requires every property to be
/// listed as required; `clarifying_question` is nullable instead.
pub fn proposal_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "relation": {
                "type": "string",
                "enum": ["SUPERSEDES", "CONTRADICTS", "INDEPENDENT", "AMBIGUOUS"]
            },
            "winning_entry_id": {"type": "string"},
            "proposed_resolution": {
                "type": "string",
                "enum": ["LAST_WRITE_WINS", "COMPENSATE", "ROLLBACK", "ESCALATE", "QUARANTINE"]
            },
            "confidence": {"type": "number", "minimum": 0, "maximum": 1},
            "rationale": {"type": "string"},
            "clarifying_question": {
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {
                            "question_text": {"type": "string"},
                            "options": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["question_text", "options"],
                        "additionalProperties": false
                    },
                    {"type": "null"}
                ]
            }
        },
        "required": [
            "relation", "winning_entry_id", "proposed_resolution",
            "confidence", "rationale", "clarifying_question"
        ],
        "additionalProperties": false
    })
}

/// Configuration for [`ConstrainedMediator`]. No provider is hardcoded: any
/// OpenAI-compatible chat completions endpoint works (vLLM, Ollama, llama.cpp
/// server, a hosted provider).
#[derive(Debug, Clone, PartialEq)]
pub struct ConstrainedMediatorConfig {
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
    /// control. Mediator default is `high` — cold-path evaluation needs deep
    /// reasoning.
    pub reasoning_effort: String,
    pub max_tokens: u32,
}

impl ConstrainedMediatorConfig {
    /// Resolve config from the environment: `OPENAI_BASE_URL` (required),
    /// `OPENAI_API_KEY` (optional), `FABRIC_MEDIATOR_MODEL` (optional; falls
    /// back to `FABRIC_DECODER_MODEL`, then the default),
    /// `FABRIC_MEDIATOR_TIMEOUT_MS`, `FABRIC_MEDIATOR_TEMPERATURE`,
    /// `FABRIC_MEDIATOR_TOP_K`, `FABRIC_MEDIATOR_TOP_P`,
    /// `FABRIC_MEDIATOR_REASONING_EFFORT`, `FABRIC_MEDIATOR_MAX_TOKENS`
    /// (all optional).
    pub fn from_env() -> Result<Self, MediatorError> {
        let model = std::env::var("FABRIC_MEDIATOR_MODEL")
            .ok()
            .or_else(|| std::env::var("FABRIC_DECODER_MODEL").ok());
        Self::resolve(
            std::env::var("OPENAI_BASE_URL").ok(),
            std::env::var("OPENAI_API_KEY").ok(),
            model,
            std::env::var("FABRIC_MEDIATOR_TIMEOUT_MS").ok(),
            std::env::var("FABRIC_MEDIATOR_TEMPERATURE").ok(),
            std::env::var("FABRIC_MEDIATOR_TOP_K").ok(),
            std::env::var("FABRIC_MEDIATOR_TOP_P").ok(),
            std::env::var("FABRIC_MEDIATOR_REASONING_EFFORT").ok(),
            std::env::var("FABRIC_MEDIATOR_MAX_TOKENS").ok(),
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
    ) -> Result<Self, MediatorError> {
        let base_url = base_url
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| MediatorError::Config("OPENAI_BASE_URL is not set".into()))?;
        let model = model
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let timeout_ms = match timeout_ms {
            Some(raw) => raw
                .trim()
                .parse::<u64>()
                .map_err(|_| MediatorError::Config(format!("invalid timeout '{raw}'")))?,
            None => DEFAULT_TIMEOUT_MS,
        };
        let temperature = match temperature {
            Some(raw) => raw
                .trim()
                .parse::<f64>()
                .map_err(|_| MediatorError::Config(format!("invalid temperature '{raw}'")))?,
            None => DEFAULT_TEMPERATURE,
        };
        let top_k = match top_k {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| MediatorError::Config(format!("invalid top_k '{raw}'")))?,
            None => DEFAULT_TOP_K,
        };
        let top_p = match top_p {
            Some(raw) => raw
                .trim()
                .parse::<f64>()
                .map_err(|_| MediatorError::Config(format!("invalid top_p '{raw}'")))?,
            None => DEFAULT_TOP_P,
        };
        let reasoning_effort = match reasoning_effort {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(MediatorError::Config("invalid reasoning_effort ''".into()));
                }
                trimmed.to_string()
            }
            None => DEFAULT_REASONING_EFFORT.to_string(),
        };
        let max_tokens = match max_tokens {
            Some(raw) => raw
                .trim()
                .parse::<u32>()
                .map_err(|_| MediatorError::Config(format!("invalid max_tokens '{raw}'")))?,
            None => DEFAULT_MAX_TOKENS,
        };
        Ok(ConstrainedMediatorConfig {
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

/// Reference Tier 3 mediator: a reasoning-capable model behind an
/// OpenAI-compatible endpoint, constrained to the proposal schema, parsed by
/// [`parse_proposal`]. Swappable with [`crate::mediator::StubMediator`]
/// behind the same trait.
pub struct ConstrainedMediator {
    http: reqwest::Client,
    config: ConstrainedMediatorConfig,
}

impl ConstrainedMediator {
    pub fn new(config: ConstrainedMediatorConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    pub fn from_env() -> Result<Self, MediatorError> {
        Ok(Self::new(ConstrainedMediatorConfig::from_env()?))
    }

    pub fn config(&self) -> &ConstrainedMediatorConfig {
        &self.config
    }

    /// Build the chat-completions request body. When `constrained` is true
    /// the body carries `response_format: json_schema` for structured-output
    /// decoding; when false (fallback path) the system prompt alone carries
    /// the "exactly one JSON object" contract.
    fn request_body(&self, input: &MediatorInput, constrained: bool) -> serde_json::Value {
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
                    "name": "resolution_proposal",
                    "strict": true,
                    "schema": proposal_json_schema()
                }
            });
        }
        body
    }

    /// POST one chat completion and return the raw content string.
    async fn complete(&self, body: &serde_json::Value) -> Result<String, MediatorError> {
        let url = self.config.completions_url();
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let mut req = self.http.post(&url).json(body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        let response = tokio::time::timeout(timeout, req.send())
            .await
            .map_err(|_| MediatorError::Timeout(self.config.timeout_ms))?
            .map_err(|e| MediatorError::Http(format!("request failed: {e}")))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| MediatorError::Http(format!("failed to read response body: {e}")))?;
        if !status.is_success() {
            return Err(MediatorError::Http(format!(
                "endpoint returned {status}: {text}"
            )));
        }
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| MediatorError::Http(format!("response is not JSON: {e}")))?;
        json.pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                MediatorError::Http("response missing choices[0].message.content".into())
            })
    }

    /// True when an endpoint error looks like "response_format/json_schema
    /// not supported" — the signal to retry unconstrained.
    fn is_schema_unsupported(err: &MediatorError) -> bool {
        match err {
            MediatorError::Http(msg) => {
                let m = msg.to_lowercase();
                m.contains("response_format") || m.contains("json_schema")
            }
            _ => false,
        }
    }
}

impl std::fmt::Debug for ConstrainedMediator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConstrainedMediator")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait]
impl ConflictMediator for ConstrainedMediator {
    async fn resolve(&self, input: MediatorInput) -> Result<ResolutionProposal, MediatorError> {
        // Preferred path: JSON-schema-constrained decoding.
        let constrained = self.request_body(&input, true);
        let raw = match self.complete(&constrained).await {
            Ok(raw) => raw,
            Err(e) if Self::is_schema_unsupported(&e) => {
                // Fallback: no response_format; the system prompt's "exactly
                // one JSON object" contract plus parse_proposal's tolerance.
                let plain = self.request_body(&input, false);
                self.complete(&plain).await?
            }
            Err(e) => return Err(e),
        };
        parse_proposal(&raw, &input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::DecoderInput;
    use crate::mediator::VerdictView;

    #[test]
    fn resolve_requires_base_url_and_defaults_model() {
        assert!(matches!(
            ConstrainedMediatorConfig::resolve(
                None, None, None, None, None, None, None, None, None
            ),
            Err(MediatorError::Config(_))
        ));
        let cfg = ConstrainedMediatorConfig::resolve(
            Some("http://localhost:8000".into()),
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
        let cfg = ConstrainedMediatorConfig::resolve(
            Some("http://x".into()),
            None,
            Some("qwen".into()),
            Some("5000".into()),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.model, "qwen");
        assert_eq!(cfg.timeout_ms, 5000);
    }

    #[test]
    fn resolve_parses_generation_params() {
        let cfg = ConstrainedMediatorConfig::resolve(
            Some("http://x".into()),
            None,
            None,
            None,
            Some("0.4".into()),
            Some("50".into()),
            Some("0.95".into()),
            Some("medium".into()),
            Some("4096".into()),
        )
        .unwrap();
        assert_eq!(cfg.temperature, 0.4);
        assert_eq!(cfg.top_k, 50);
        assert_eq!(cfg.top_p, 0.95);
        assert_eq!(cfg.reasoning_effort, "medium");
        assert_eq!(cfg.max_tokens, 4096);
        assert!(ConstrainedMediatorConfig::resolve(
            Some("http://x".into()),
            None,
            None,
            None,
            None,
            Some("not-a-number".into()),
            None,
            None,
            None,
        )
        .is_err());
    }

    #[test]
    fn completions_url_handles_both_base_conventions() {
        let bare = ConstrainedMediatorConfig {
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
        let v1 = ConstrainedMediatorConfig {
            base_url: "https://api.example.com/v1".into(),
            ..bare
        };
        assert_eq!(
            v1.completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    fn sample_input() -> MediatorInput {
        serde_json::from_value(serde_json::json!({
            "verdict": {"relation": "SUPERSEDES", "shared_entities": [], "confidence": 0.96,
                        "explanation": "clean revision"},
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

    fn test_mediator() -> ConstrainedMediator {
        ConstrainedMediator::new(ConstrainedMediatorConfig {
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
    fn mediator_input_deserializes_flat_with_verdict() {
        let input = sample_input();
        assert_eq!(input.verdict.relation, "SUPERSEDES");
        assert_eq!(input.input.entry_id_a, "a");
        assert_eq!(input.tool_category, None);
    }

    #[test]
    fn constrained_body_carries_json_schema_and_prompt() {
        let body = test_mediator().request_body(&sample_input(), true);
        assert_eq!(body["model"], "qwen");
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["provider"]["sort"], "throughput");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
        assert!(body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("set_thermostat"));
        let rf = &body["response_format"];
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["name"], "resolution_proposal");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(
            rf["json_schema"]["schema"]["properties"]["proposed_resolution"]["enum"],
            serde_json::json!([
                "LAST_WRITE_WINS",
                "COMPENSATE",
                "ROLLBACK",
                "ESCALATE",
                "QUARANTINE"
            ])
        );
    }

    #[test]
    fn fallback_body_omits_response_format() {
        let body = test_mediator().request_body(&sample_input(), false);
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn schema_unsupported_detection() {
        assert!(ConstrainedMediator::is_schema_unsupported(
            &MediatorError::Http("endpoint returned 400: response_format is not supported".into())
        ));
        assert!(ConstrainedMediator::is_schema_unsupported(
            &MediatorError::Http("endpoint returned 400: unknown field json_schema".into())
        ));
        assert!(!ConstrainedMediator::is_schema_unsupported(
            &MediatorError::Http("endpoint returned 500: boom".into())
        ));
        assert!(!ConstrainedMediator::is_schema_unsupported(
            &MediatorError::Timeout(1000)
        ));
    }

    #[test]
    fn system_prompt_mirrors_output_schema_and_contract() {
        // Byte-exact mirror of the locked PROPOSAL_OUTPUT_SCHEMA const.
        assert!(SYSTEM_PROMPT.contains(crate::mediator::PROPOSAL_OUTPUT_SCHEMA));
        // The five resolutions.
        for res in [
            "LAST_WRITE_WINS",
            "COMPENSATE",
            "ROLLBACK",
            "ESCALATE",
            "QUARANTINE",
        ] {
            assert!(SYSTEM_PROMPT.contains(res), "missing {res}");
        }
        // The resolve-vs-ask judgment.
        assert!(SYSTEM_PROMPT.contains("DO NOT GUESS"));
        // The high-stakes fail-closed bias.
        assert!(SYSTEM_PROMPT.contains("Bias to fail closed on high stakes"));
        // The honest-confidence / auto-approve rationale.
        assert!(SYSTEM_PROMPT.contains("auto-approve"));
        assert!(SYSTEM_PROMPT.contains("Overconfident"));
        // The clarifying-question-as-moat framing.
        assert!(SYSTEM_PROMPT.contains("the MOAT"));
        // The anti-actuator firewall.
        assert!(SYSTEM_PROMPT.contains("You propose. You do NOT enforce"));
        assert!(SYSTEM_PROMPT.contains("You do NOT invent entry IDs"));
        assert!(SYSTEM_PROMPT.contains("EXACTLY one JSON object"));
        // Few-shot examples present.
        assert!(SYSTEM_PROMPT.contains("Example 5"));
    }

    #[test]
    fn verdict_view_roundtrip_serde() {
        let v = VerdictView {
            relation: "AMBIGUOUS".into(),
            shared_entities: vec![],
            confidence: 0.4,
            explanation: String::new(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: VerdictView = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn decoder_input_still_deserializes_in_flatten() {
        let input: MediatorInput = serde_json::from_value(serde_json::json!({
            "verdict": {"relation": "CONTRADICTS", "confidence": 0.8, "explanation": "x"},
            "session_id": "s1",
            "entry_id_a": "a",
            "entry_id_b": "b",
            "call_a": {"tool_name": "t", "target": "x", "params": {}, "idempotency_key": ""},
            "call_b": {"tool_name": "t", "target": "x", "params": {}, "idempotency_key": ""},
            "context": [],
            "tool_category": "financial"
        }))
        .unwrap();
        assert_eq!(input.tool_category.as_deref(), Some("financial"));
        let _: DecoderInput = input.input;
    }
}
