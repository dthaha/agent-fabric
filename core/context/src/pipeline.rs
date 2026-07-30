//! Reference four-tier conflict pipeline: the orchestration glue that wires
//! Tier 1 (structural detector) output through Tier 2 (decoder), Tier 3
//! (mediator), and Tier 4 ([`ConflictResolver`] policy veto) into a single
//! [`FinalDecision`]. Thin by design — no new logic lives here; each tier is
//! independently swappable (stubs today, post-trained models later).
//!
//! Path: a [`StructuralConflict`] from [`crate::conflict::detect_in_region`]
//! with disposition Escalate is decoded, mediated, and decided. An
//! idempotent LastWriteWins pair skips the model tiers (its deterministic
//! proposal is synthesized at full confidence) but still passes through the
//! Tier 4 veto — policy decides every outcome.
//!
//! Clarifying questions are routed to the surface holding presence: the
//! active lease holder from [`crate::store::LeaseAuthority::active_lease`]. Actual delivery
//! (queue when no surface is present, surface on next activity) is a later
//! concern; the pipeline produces the routed [`FinalDecision::Ask`].

use fabric_policy::conflict::{
    CompensationCapability, ConflictPolicySet, ConflictResolver, FinalDecision,
};
use fabric_types::conflict::{ConflictResolution, ResolutionProposal};
use thiserror::Error;

use crate::conflict::{StructuralConflict, StructuralDisposition};
use crate::decoder::{build_decoder_input, ConflictDecoder, DecoderError};
use crate::mediator::{ConflictMediator, MediatorError, MediatorInput};
use crate::store::{ContextStore, LeaseAuthority};

/// Errors from the reference pipeline: whichever tier failed.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("decoder tier: {0}")]
    Decoder(#[from] DecoderError),
    #[error("mediator tier: {0}")]
    Mediator(#[from] MediatorError),
}

/// The default number of entries preceding the conflict handed to the
/// decoder as conversation context.
pub const DEFAULT_CONTEXT_WINDOW: u64 = 20;

/// Wires decoder + mediator + policy set + compensation capability into one
/// end-to-end conflict resolution call. Generic over the two model seams so
/// it runs with stubs (tests) or real impls (production).
pub struct ConflictPipeline<D, M> {
    decoder: D,
    mediator: M,
    policies: ConflictPolicySet,
    compensation: CompensationCapability,
    context_window: u64,
}

impl<D: ConflictDecoder, M: ConflictMediator> ConflictPipeline<D, M> {
    pub fn new(
        decoder: D,
        mediator: M,
        policies: ConflictPolicySet,
        compensation: CompensationCapability,
    ) -> Self {
        Self {
            decoder,
            mediator,
            policies,
            compensation,
            context_window: DEFAULT_CONTEXT_WINDOW,
        }
    }

    pub fn with_context_window(mut self, window: u64) -> Self {
        self.context_window = window;
        self
    }

    /// Run one Tier 1 conflict through tiers 2-4 and return the final
    /// decision. `tool_category` selects the governing [`ConflictPolicy`]
    /// (exact match, then the `"*"` org default, deny-wins).
    pub async fn resolve_conflict(
        &self,
        store: &(impl ContextStore + LeaseAuthority),
        conflict: &StructuralConflict,
        tool_category: &str,
    ) -> Result<FinalDecision, PipelineError> {
        let proposal = match conflict.disposition {
            // Idempotent pair: the detector already resolved it
            // deterministically. Synthesize the proposal at full confidence;
            // the policy veto still applies.
            StructuralDisposition::LastWriteWins => ResolutionProposal {
                session_id: conflict.session_id.clone(),
                relation: conflict.relation as i32,
                winning_entry_id: conflict.lww_winner_entry_id.clone().unwrap_or_default(),
                proposed_resolution: ConflictResolution::LastWriteWins as i32,
                confidence: 1.0,
                rationale: "structural: idempotent pair, deterministic last-write-wins".into(),
                clarifying_question: None,
            },
            // Mutating collision: Tier 2 classifies, Tier 3 proposes.
            StructuralDisposition::Escalate => {
                let input = build_decoder_input(store, conflict, self.context_window).await?;
                let verdict = self.decoder.decode(input.clone()).await?;
                let mut mediator_input = MediatorInput::new(&verdict, input);
                mediator_input.tool_category = Some(tool_category.to_string());
                self.mediator.resolve(mediator_input).await?
            }
        };

        // Clarifying questions route to the surface holding presence: the
        // active lease holder. Empty when no surface is present. A store
        // failure is logged, never silently swallowed.
        let route_to = match store.active_lease(&conflict.session_id).await {
            Ok(Some(lease)) => lease.holder_id,
            Ok(None) => String::new(),
            Err(e) => {
                tracing::warn!(
                    session = %conflict.session_id,
                    error = %e,
                    "active lease lookup failed; clarifying question routes nowhere"
                );
                String::new()
            }
        };

        let policy = self.policies.policy_for(tool_category);
        Ok(ConflictResolver::decide(
            &proposal,
            &policy,
            self.compensation,
            &route_to,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::{detect_pair, StructuralVerdict};
    use crate::db::ms_to_timestamp;
    use crate::db::tests::{test_lease, test_session};
    use crate::db::SqliteContextStore;
    use crate::decoder::StubDecoder;
    use crate::mediator::StubMediator;
    use crate::tool_call;
    use async_trait::async_trait;
    use fabric_types::conflict::{ClarifyingQuestion, ConflictRelation};
    use fabric_types::context::{ContextEntry, EntryKind, Locus, ToolCall};
    use std::collections::HashMap;

    fn tool_entry(
        id: &str,
        seq: u64,
        tool: &str,
        target: &str,
        params: &[(&str, &str)],
        idempotency_key: &str,
    ) -> ContextEntry {
        let call = ToolCall {
            tool_name: tool.into(),
            target: target.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
            idempotency_key: idempotency_key.into(),
        };
        ContextEntry {
            entry_id: id.into(),
            session_id: "s1".into(),
            seq,
            kind: EntryKind::ToolCall as i32,
            payload: tool_call::encode(&call),
            lease_holder: "h".into(),
            policy_version: "v1".into(),
            locus: Locus::Endpoint as i32,
            created_at: Some(ms_to_timestamp(1000 + seq as i64)),
            received_at: None,
            disposition: String::new(),
        }
    }

    fn store_with_conflict() -> (SqliteContextStore, StructuralConflict) {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        store
            .grant_lease(&test_lease("l1", "s1", "laptop-7"))
            .unwrap();
        let a = tool_entry("a", 1, "set_config", "ui.theme", &[("value", "dark")], "");
        let b = tool_entry("b", 2, "set_config", "ui.theme", &[("value", "light")], "");
        store.insert_entry_raw(&a).unwrap();
        store.insert_entry_raw(&b).unwrap();
        let StructuralVerdict::Conflict(conflict) = detect_pair(&a, &b) else {
            panic!("expected structural conflict");
        };
        (store, conflict)
    }

    #[tokio::test]
    async fn full_pipeline_with_stubs_produces_sane_decision() {
        let (store, conflict) = store_with_conflict();
        let pipeline = ConflictPipeline::new(
            StubDecoder,
            StubMediator,
            // Permissive policy so the stub mediator's zero-confidence
            // ESCALATE proposal flows to the confidence gate, not a forced
            // veto.
            ConflictPolicySet::new(vec![fabric_types::conflict::ConflictPolicy {
                tool_category: "*".into(),
                resolution: ConflictResolution::LastWriteWins as i32,
                auto_approve_threshold: 0.8,
                require_compensation_support: true,
                fallback: ConflictResolution::Quarantine as i32,
            }]),
            CompensationCapability::unsupported(),
        );
        let decision = pipeline
            .resolve_conflict(&store, &conflict, "config")
            .await
            .unwrap();
        // Stub mediator proposes ESCALATE at confidence 0.0 (< 0.8):
        // under-confidence never auto-applies -> Escalate.
        assert!(matches!(decision, FinalDecision::Escalate { .. }));
    }

    #[tokio::test]
    async fn idempotent_pair_flows_through_veto_to_apply() {
        let store = SqliteContextStore::open_in_memory().unwrap();
        store.create_session(&test_session("s1")).unwrap();
        let a = tool_entry("a", 1, "cache_put", "k1", &[("v", "1")], "key-a");
        let b = tool_entry("b", 2, "cache_put", "k1", &[("v", "2")], "key-b");
        store.insert_entry_raw(&a).unwrap();
        store.insert_entry_raw(&b).unwrap();
        let StructuralVerdict::Conflict(conflict) = detect_pair(&a, &b) else {
            panic!("expected structural conflict");
        };
        assert_eq!(conflict.disposition, StructuralDisposition::LastWriteWins);

        let pipeline = ConflictPipeline::new(
            StubDecoder,
            StubMediator,
            ConflictPolicySet::new(vec![fabric_types::conflict::ConflictPolicy {
                tool_category: "*".into(),
                resolution: ConflictResolution::LastWriteWins as i32,
                auto_approve_threshold: 0.8,
                require_compensation_support: true,
                fallback: ConflictResolution::Quarantine as i32,
            }]),
            CompensationCapability::unsupported(),
        );
        let decision = pipeline
            .resolve_conflict(&store, &conflict, "config")
            .await
            .unwrap();
        assert_eq!(
            decision,
            FinalDecision::Apply {
                resolution: ConflictResolution::LastWriteWins,
                winning_entry_id: "b".into(),
            }
        );
    }

    struct AskingMediator;

    #[async_trait]
    impl ConflictMediator for AskingMediator {
        async fn resolve(&self, input: MediatorInput) -> Result<ResolutionProposal, MediatorError> {
            Ok(ResolutionProposal {
                session_id: input.input.session_id,
                relation: ConflictRelation::Ambiguous as i32,
                winning_entry_id: String::new(),
                proposed_resolution: ConflictResolution::Escalate as i32,
                confidence: 0.0,
                rationale: "cannot tell which theme the user wants".into(),
                clarifying_question: Some(ClarifyingQuestion {
                    question_text: "dark or light theme?".into(),
                    options: vec!["dark".into(), "light".into()],
                }),
            })
        }
    }

    #[tokio::test]
    async fn clarifying_question_routes_to_presence_holder() {
        let (store, conflict) = store_with_conflict();
        let pipeline = ConflictPipeline::new(
            StubDecoder,
            AskingMediator,
            ConflictPolicySet::new(vec![]),
            CompensationCapability::unsupported(),
        );
        let decision = pipeline
            .resolve_conflict(&store, &conflict, "config")
            .await
            .unwrap();
        // The active lease is held by "laptop-7" — the presence surface.
        assert_eq!(
            decision,
            FinalDecision::Ask {
                question: ClarifyingQuestion {
                    question_text: "dark or light theme?".into(),
                    options: vec!["dark".into(), "light".into()],
                },
                route_to: "laptop-7".into(),
            }
        );
    }

    #[tokio::test]
    async fn ask_with_no_present_surface_routes_to_empty() {
        let (store, conflict) = store_with_conflict();
        store.release_lease("s1", "laptop-7").unwrap();
        let pipeline = ConflictPipeline::new(
            StubDecoder,
            AskingMediator,
            ConflictPolicySet::new(vec![]),
            CompensationCapability::unsupported(),
        );
        let decision = pipeline
            .resolve_conflict(&store, &conflict, "config")
            .await
            .unwrap();
        let FinalDecision::Ask { route_to, .. } = decision else {
            panic!("expected Ask, got {decision:?}");
        };
        assert_eq!(route_to, "");
    }
}
