//! Policy-aware wrapper — the final safety net. The pipeline is:
//!
//! 1. Model (Phase 5) produces a ModelAdvisory (semantic estimate)
//! 2. RulesClassifier validates against hard constraints, uses the
//!    advisory if present, falls back to heuristics if not
//! 3. PolicyAwareClassifier re-checks the result against the effective
//!    policy and downgrades server/split → endpoint when policy forbids it
//!
//! The model informs, the rules decide, the policy vetoes.

use fabric_policy::{Decision, PolicyGate};
use fabric_types::context::Locus;

use crate::{ClassifyInput, LocusClassifier, LocusDecision};

/// Wraps any [`LocusClassifier`] with a [`PolicyGate`]. Server and split
/// decisions are validated before they are returned; endpoint decisions
/// pass through untouched.
pub struct PolicyAwareClassifier<C: LocusClassifier> {
    inner: C,
    gate: PolicyGate,
}

impl<C: LocusClassifier> PolicyAwareClassifier<C> {
    pub fn new(inner: C, gate: PolicyGate) -> Self {
        Self { inner, gate }
    }

    pub fn inner(&self) -> &C {
        &self.inner
    }

    /// Force the turn local, recording why the original locus was vetoed.
    fn downgrade(decision: &LocusDecision, why: &str) -> LocusDecision {
        LocusDecision {
            locus: Locus::Endpoint,
            reason: format!("{}; downgraded to endpoint: {why}", decision.reason),
            confidence: decision.confidence,
            fallback: None,
        }
    }
}

impl<C: LocusClassifier> LocusClassifier for PolicyAwareClassifier<C> {
    fn classify(&self, input: &ClassifyInput) -> LocusDecision {
        let decision = self.inner.classify(input);
        if !matches!(decision.locus, Locus::Server | Locus::Split) {
            return decision;
        }
        // The gate denies everything while the kill switch is engaged.
        if self.gate.is_killed() {
            return Self::downgrade(&decision, "kill switch engaged");
        }
        // Server-side inference needs at least one provider rule; without one the
        // gate fails closed on every request, so server is not available.
        if self.gate.effective().inference_rules.is_empty() {
            return Self::downgrade(&decision, "no server-side inference rules in policy");
        }
        // Every data class touched by this turn must be allowed to reach the
        // server destination. RequireApproval blocks egress here too: the
        // classifier is a synchronous path with no approval round-trip, so
        // "needs approval" means "not now" — stay on the endpoint.
        for class in &input.data_classes {
            match self.gate.check_data_egress(class, "server") {
                Decision::Deny(reason) | Decision::RequireApproval(reason) => {
                    return Self::downgrade(&decision, &reason);
                }
                Decision::Allow => {}
            }
        }
        decision
    }
}
