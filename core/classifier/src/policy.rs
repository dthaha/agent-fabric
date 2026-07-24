//! Policy-aware wrapper. A rules engine can optimistically pick hosted or
//! split, but policy is deny-wins: this wrapper re-checks every off-device
//! decision against the effective policy and downgrades to the endpoint
//! when the gate would refuse it.

use fabric_policy::{Decision, PolicyGate};
use fabric_types::context::Locus;

use crate::{ClassifyInput, LocusClassifier, LocusDecision};

/// Wraps any [`LocusClassifier`] with a [`PolicyGate`]. Hosted and split
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
        if !matches!(decision.locus, Locus::Hosted | Locus::Split) {
            return decision;
        }
        // The gate denies everything while the kill switch is engaged.
        if self.gate.is_killed() {
            return Self::downgrade(&decision, "kill switch engaged");
        }
        // Hosted inference needs at least one provider rule; without one the
        // gate fails closed on every request, so hosted is not available.
        if self.gate.effective().inference_rules.is_empty() {
            return Self::downgrade(&decision, "no hosted inference rules in policy");
        }
        // Every data class touched by this turn must be allowed to reach the
        // hosted destination.
        for class in &input.data_classes {
            if let Decision::Deny(reason) = self.gate.check_data_egress(class, "hosted") {
                return Self::downgrade(&decision, &reason);
            }
        }
        decision
    }
}
