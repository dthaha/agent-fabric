//! Tier 4 conflict policy veto. Pure and deterministic — no I/O, no clock,
//! no randomness, no model. The veto consumes a Tier 3
//! [`ResolutionProposal`] plus the governing [`ConflictPolicy`] and emits a
//! [`FinalDecision`]. It decides; it never executes side effects and never
//! mutates the op-log. Policy holds the veto: the resolver honors, gates, or
//! vetoes the mediator's proposal — it never invents a different resolution.
//!
//! This module also hosts the [`ConflictPolicySet`] lookup (per-tool-category
//! policy with an org-default `"*"`) and its deny-wins merge. It lives in
//! `core/policy` (not `core/context`) because it IS policy: it mirrors the
//! endpoint/server deny-wins philosophy of [`crate::merge`], and it keeps the
//! context plane free of policy semantics. `core/types` stays gen-only.
//!
//! Decision priority (in order; the first match wins):
//! 1. **Clarifying question short-circuits.** If the mediator asked a
//!    question, the decision is [`FinalDecision::Ask`] routed to the surface
//!    holding presence (the active lease holder). Policy honors the ask.
//! 2. **Policy-forced QUARANTINE/ESCALATE (deny-wins).** If the governing
//!    policy's resolution is QUARANTINE or ESCALATE for this category, it is
//!    honored regardless of the proposal or its confidence.
//! 3. **Confidence gate.** Below the auto-approve threshold the proposal
//!    never auto-applies: Escalate normally, Quarantine for high-stakes
//!    categories (fail closed).
//! 4. **Compensation feasibility.** A COMPENSATE/ROLLBACK proposal is never
//!    approved when the policy requires compensation support and the tool
//!    lacks it — the policy fallback applies instead.
//! 5. **High-stakes fail-closed.** High-stakes categories (financial,
//!    deployment, irreversible) require [`HIGH_STAKES_THRESHOLD`] confidence
//!    to auto-apply; when in doubt, Quarantine.
//! 6. **Apply.** The proposed resolution is approved; the caller executes
//!    the side effect.

use fabric_types::conflict::{
    ClarifyingQuestion, ConflictPolicy, ConflictResolution, ResolutionProposal,
};

/// Default confidence threshold for the built-in org policy.
pub const DEFAULT_AUTO_APPROVE_THRESHOLD: f32 = 0.8;

/// The higher confidence bar high-stakes categories must clear to
/// auto-apply. A wrong auto-resolution on an irreversible action is the
/// worst failure mode.
pub const HIGH_STAKES_THRESHOLD: f32 = 0.95;

/// Tool categories whose wrong auto-resolution is unrecoverable.
const HIGH_STAKES_CATEGORIES: [&str; 3] = ["financial", "deployment", "irreversible"];

/// Whether a tool category is high-stakes (fail-closed bias).
pub fn is_high_stakes(tool_category: &str) -> bool {
    HIGH_STAKES_CATEGORIES.contains(&tool_category)
}

/// Restrictiveness ordering for deny-wins merges: QUARANTINE (most
/// restrictive) beats ESCALATE beats ROLLBACK beats COMPENSATE beats
/// LAST_WRITE_WINS (most permissive).
fn restrictiveness(resolution: ConflictResolution) -> u8 {
    match resolution {
        ConflictResolution::Unspecified => 0,
        ConflictResolution::LastWriteWins => 1,
        ConflictResolution::Compensate => 2,
        ConflictResolution::Rollback => 3,
        ConflictResolution::Escalate => 4,
        ConflictResolution::Quarantine => 5,
    }
}

fn resolution_of(raw: i32) -> ConflictResolution {
    ConflictResolution::try_from(raw).unwrap_or(ConflictResolution::Unspecified)
}

/// The built-in default policy used when no configured policy governs a
/// category: resolution ESCALATE, threshold 0.8, compensation support
/// required for COMPENSATE/ROLLBACK, fallback QUARANTINE. Fail-closed.
pub fn default_policy() -> ConflictPolicy {
    ConflictPolicy {
        tool_category: "*".into(),
        resolution: ConflictResolution::Escalate as i32,
        auto_approve_threshold: DEFAULT_AUTO_APPROVE_THRESHOLD,
        require_compensation_support: true,
        fallback: ConflictResolution::Quarantine as i32,
    }
}

/// Deny-wins merge of two conflict policies: the MORE RESTRICTIVE governs.
/// Higher auto-approve threshold wins; QUARANTINE/ESCALATE beat auto-apply
/// resolutions; `require_compensation_support` is OR-ed (true wins); the
/// more restrictive fallback wins. Mirrors the endpoint/server deny-wins
/// merge in [`crate::merge`].
pub fn merge_policies(a: &ConflictPolicy, b: &ConflictPolicy) -> ConflictPolicy {
    let pick = |x: i32, y: i32| {
        if restrictiveness(resolution_of(x)) >= restrictiveness(resolution_of(y)) {
            x
        } else {
            y
        }
    };
    ConflictPolicy {
        tool_category: a.tool_category.clone(),
        resolution: pick(a.resolution, b.resolution),
        auto_approve_threshold: a.auto_approve_threshold.max(b.auto_approve_threshold),
        require_compensation_support: a.require_compensation_support
            || b.require_compensation_support,
        fallback: pick(a.fallback, b.fallback),
    }
}

/// A set of configured conflict policies with lookup by tool category.
/// Exact category match first, then the `"*"` org default; when both apply
/// they are merged deny-wins. When neither exists the built-in
/// [`default_policy`] governs, so the resolver always has something to
/// evaluate against.
#[derive(Debug, Clone, Default)]
pub struct ConflictPolicySet {
    policies: Vec<ConflictPolicy>,
}

impl ConflictPolicySet {
    pub fn new(policies: Vec<ConflictPolicy>) -> Self {
        Self { policies }
    }

    /// The governing policy for a tool category. The returned policy's
    /// `tool_category` is the REQUESTED category (not `"*"`) so downstream
    /// high-stakes checks see the real category even when the org default
    /// supplied the values.
    pub fn policy_for(&self, tool_category: &str) -> ConflictPolicy {
        let exact = self
            .policies
            .iter()
            .find(|p| p.tool_category == tool_category);
        let org_default = self.policies.iter().find(|p| p.tool_category == "*");
        let mut policy = match (exact, org_default) {
            (Some(e), Some(d)) => merge_policies(e, d),
            (Some(e), None) => e.clone(),
            (None, Some(d)) => d.clone(),
            (None, None) => default_policy(),
        };
        policy.tool_category = tool_category.to_string();
        policy
    }
}

/// Whether the relevant tool can perform a compensation, derived by the
/// caller from `Tool::supports_compensation`. Keeps the resolver decoupled
/// from the tool registry — the resolver never touches tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompensationCapability {
    pub supports: bool,
}

impl CompensationCapability {
    pub fn supported() -> Self {
        Self { supports: true }
    }

    pub fn unsupported() -> Self {
        Self { supports: false }
    }
}

/// The final, decided action for a conflict. The resolver emits this and
/// nothing else — execution is the caller's concern.
#[derive(Debug, Clone, PartialEq)]
pub enum FinalDecision {
    /// Policy approves the proposal; the caller executes the side effect.
    Apply {
        resolution: ConflictResolution,
        winning_entry_id: String,
    },
    /// The mediator asked a clarifying question; route it to the surface
    /// holding presence (the active lease holder). Empty `route_to` means no
    /// surface is present — queue and surface on next activity (delivery is
    /// a later concern).
    Ask {
        question: ClarifyingQuestion,
        route_to: String,
    },
    /// Route to a human / higher authority.
    Escalate { reason: String },
    /// Fail closed: act on neither entry.
    Quarantine { reason: String },
}

/// The Tier 4 policy veto. Stateless and pure: same input, same decision.
pub struct ConflictResolver;

impl ConflictResolver {
    /// Decide the final action for a mediator proposal under the governing
    /// policy. `route_to` is the surface holding presence (the active lease
    /// holder), used only when the decision is [`FinalDecision::Ask`].
    pub fn decide(
        proposal: &ResolutionProposal,
        policy: &ConflictPolicy,
        compensation: CompensationCapability,
        route_to: &str,
    ) -> FinalDecision {
        // 1. Clarifying question short-circuits: the mediator asked; policy
        //    honors it. Routed to the surface holding presence.
        if let Some(question) = &proposal.clarifying_question {
            return FinalDecision::Ask {
                question: question.clone(),
                route_to: route_to.to_string(),
            };
        }

        // 2. Deny-wins / fail-closed default: the governing policy can force
        //    QUARANTINE or ESCALATE regardless of the proposal or confidence.
        match resolution_of(policy.resolution) {
            ConflictResolution::Quarantine => {
                return FinalDecision::Quarantine {
                    reason: format!(
                        "policy forces QUARANTINE for category '{}'",
                        policy.tool_category
                    ),
                };
            }
            ConflictResolution::Escalate => {
                return FinalDecision::Escalate {
                    reason: format!(
                        "policy forces ESCALATE for category '{}'",
                        policy.tool_category
                    ),
                };
            }
            _ => {}
        }

        let proposed = resolution_of(proposal.proposed_resolution);
        let high_stakes = is_high_stakes(&policy.tool_category);

        // Sanitize confidence: a NaN would sail past every comparison
        // (`NaN < threshold` is false) and auto-apply. Non-finite values are
        // treated as zero confidence — fail closed.
        let confidence = if proposal.confidence.is_finite() {
            proposal.confidence
        } else {
            0.0
        };

        // 3 + 5. Confidence gate, with the high-stakes fail-closed bar:
        //    under-confidence never auto-applies. High-stakes categories must
        //    clear the higher bar, and when in doubt they Quarantine.
        let threshold = if high_stakes {
            policy.auto_approve_threshold.max(HIGH_STAKES_THRESHOLD)
        } else {
            policy.auto_approve_threshold
        };
        if confidence < threshold {
            if high_stakes {
                return FinalDecision::Quarantine {
                    reason: format!(
                        "confidence {:.2} below high-stakes bar {:.2} for category '{}'; fail closed",
                        confidence, threshold, policy.tool_category
                    ),
                };
            }
            return FinalDecision::Escalate {
                reason: format!(
                    "confidence {:.2} below auto-approve threshold {:.2}",
                    confidence, threshold
                ),
            };
        }

        // 4. Compensation feasibility: never approve an undo the tool cannot
        //    perform. The policy fallback applies instead.
        if matches!(
            proposed,
            ConflictResolution::Compensate | ConflictResolution::Rollback
        ) && policy.require_compensation_support
            && !compensation.supports
        {
            let fallback = resolution_of(policy.fallback);
            return match fallback {
                ConflictResolution::Quarantine => FinalDecision::Quarantine {
                    reason: format!(
                        "{proposed:?} proposed but tool lacks compensation support; fallback QUARANTINE"
                    ),
                },
                other => FinalDecision::Escalate {
                    reason: format!(
                        "{proposed:?} proposed but tool lacks compensation support; fallback {other:?}"
                    ),
                },
            };
        }

        // 6. Otherwise honor the proposal. The resolver never invents a
        //    different resolution; a proposed ESCALATE/QUARANTINE is honored
        //    as-is, and UNSPECIFIED fails closed.
        match proposed {
            ConflictResolution::Escalate => FinalDecision::Escalate {
                reason: format!("mediator proposed ESCALATE: {}", proposal.rationale),
            },
            ConflictResolution::Quarantine => FinalDecision::Quarantine {
                reason: format!("mediator proposed QUARANTINE: {}", proposal.rationale),
            },
            ConflictResolution::Unspecified => FinalDecision::Quarantine {
                reason: "proposal has no concrete resolution; fail closed".into(),
            },
            resolution => FinalDecision::Apply {
                resolution,
                winning_entry_id: proposal.winning_entry_id.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::conflict::ConflictRelation;

    fn proposal(
        resolution: ConflictResolution,
        confidence: f32,
        winner: &str,
    ) -> ResolutionProposal {
        ResolutionProposal {
            session_id: "s1".into(),
            relation: ConflictRelation::Supersedes as i32,
            winning_entry_id: winner.into(),
            proposed_resolution: resolution as i32,
            confidence,
            rationale: "test".into(),
            clarifying_question: None,
        }
    }

    fn policy(
        category: &str,
        resolution: ConflictResolution,
        threshold: f32,
        require_comp: bool,
        fallback: ConflictResolution,
    ) -> ConflictPolicy {
        ConflictPolicy {
            tool_category: category.into(),
            resolution: resolution as i32,
            auto_approve_threshold: threshold,
            require_compensation_support: require_comp,
            fallback: fallback as i32,
        }
    }

    fn permissive() -> ConflictPolicy {
        policy(
            "filesystem",
            ConflictResolution::LastWriteWins,
            0.5,
            true,
            ConflictResolution::Escalate,
        )
    }

    #[test]
    fn clean_supersedes_high_confidence_permissive_applies() {
        let p = proposal(ConflictResolution::LastWriteWins, 0.95, "b");
        let d = ConflictResolver::decide(
            &p,
            &permissive(),
            CompensationCapability::unsupported(),
            "holder-1",
        );
        assert_eq!(
            d,
            FinalDecision::Apply {
                resolution: ConflictResolution::LastWriteWins,
                winning_entry_id: "b".into(),
            }
        );
    }

    #[test]
    fn below_threshold_escalates_not_applies() {
        let p = proposal(ConflictResolution::LastWriteWins, 0.4, "b");
        let d = ConflictResolver::decide(
            &p,
            &permissive(),
            CompensationCapability::unsupported(),
            "holder-1",
        );
        assert!(matches!(d, FinalDecision::Escalate { .. }));
    }

    #[test]
    fn compensate_without_tool_support_falls_back() {
        let p = proposal(ConflictResolution::Compensate, 0.99, "b");
        // require_compensation_support = true, tool lacks support, fallback
        // QUARANTINE.
        let pol = policy(
            "filesystem",
            ConflictResolution::LastWriteWins,
            0.5,
            true,
            ConflictResolution::Quarantine,
        );
        let d = ConflictResolver::decide(&p, &pol, CompensationCapability::unsupported(), "h");
        assert!(matches!(d, FinalDecision::Quarantine { .. }));
        // Same, but fallback ESCALATE.
        let pol = policy(
            "filesystem",
            ConflictResolution::LastWriteWins,
            0.5,
            true,
            ConflictResolution::Escalate,
        );
        let d = ConflictResolver::decide(&p, &pol, CompensationCapability::unsupported(), "h");
        assert!(matches!(d, FinalDecision::Escalate { .. }));
    }

    #[test]
    fn compensate_with_tool_support_applies() {
        let p = proposal(ConflictResolution::Compensate, 0.99, "b");
        let d =
            ConflictResolver::decide(&p, &permissive(), CompensationCapability::supported(), "h");
        assert_eq!(
            d,
            FinalDecision::Apply {
                resolution: ConflictResolution::Compensate,
                winning_entry_id: "b".into(),
            }
        );
    }

    #[test]
    fn rollback_feasibility_mirrors_compensate() {
        let p = proposal(ConflictResolution::Rollback, 0.99, "b");
        let d = ConflictResolver::decide(
            &p,
            &permissive(),
            CompensationCapability::unsupported(),
            "h",
        );
        assert!(matches!(d, FinalDecision::Escalate { .. }));
        let d =
            ConflictResolver::decide(&p, &permissive(), CompensationCapability::supported(), "h");
        assert_eq!(
            d,
            FinalDecision::Apply {
                resolution: ConflictResolution::Rollback,
                winning_entry_id: "b".into(),
            }
        );
    }

    #[test]
    fn high_stakes_moderate_confidence_quarantines() {
        // Financial at 0.8: above a permissive threshold but below the
        // high-stakes bar -> fail closed.
        let p = proposal(ConflictResolution::LastWriteWins, 0.8, "b");
        let pol = policy(
            "financial",
            ConflictResolution::LastWriteWins,
            0.5,
            true,
            ConflictResolution::Escalate,
        );
        let d = ConflictResolver::decide(&p, &pol, CompensationCapability::unsupported(), "h");
        assert!(matches!(d, FinalDecision::Quarantine { .. }));
        // Above the high-stakes bar it applies.
        let p = proposal(ConflictResolution::LastWriteWins, 0.97, "b");
        let d = ConflictResolver::decide(&p, &pol, CompensationCapability::unsupported(), "h");
        assert!(matches!(d, FinalDecision::Apply { .. }));
    }

    #[test]
    fn clarifying_question_short_circuits_to_ask() {
        let mut p = proposal(ConflictResolution::Escalate, 0.0, "");
        p.clarifying_question = Some(ClarifyingQuestion {
            question_text: "did the cancel supersede the booking?".into(),
            options: vec!["yes".into(), "no".into()],
        });
        // Even with a policy that would force QUARANTINE, the ask wins.
        let pol = policy(
            "financial",
            ConflictResolution::Quarantine,
            0.99,
            true,
            ConflictResolution::Quarantine,
        );
        let d =
            ConflictResolver::decide(&p, &pol, CompensationCapability::unsupported(), "laptop-7");
        assert_eq!(
            d,
            FinalDecision::Ask {
                question: ClarifyingQuestion {
                    question_text: "did the cancel supersede the booking?".into(),
                    options: vec!["yes".into(), "no".into()],
                },
                route_to: "laptop-7".into(),
            }
        );
    }

    #[test]
    fn policy_forced_quarantine_wins_regardless_of_confidence() {
        let p = proposal(ConflictResolution::LastWriteWins, 1.0, "b");
        let pol = policy(
            "filesystem",
            ConflictResolution::Quarantine,
            0.1,
            false,
            ConflictResolution::Quarantine,
        );
        let d = ConflictResolver::decide(&p, &pol, CompensationCapability::supported(), "h");
        assert!(matches!(d, FinalDecision::Quarantine { .. }));
    }

    #[test]
    fn policy_forced_escalate_wins_regardless_of_confidence() {
        let p = proposal(ConflictResolution::LastWriteWins, 1.0, "b");
        let d = ConflictResolver::decide(
            &p,
            &default_policy(),
            CompensationCapability::supported(),
            "h",
        );
        assert!(matches!(d, FinalDecision::Escalate { .. }));
    }

    #[test]
    fn unspecified_proposal_fails_closed() {
        let p = proposal(ConflictResolution::Unspecified, 1.0, "b");
        let d =
            ConflictResolver::decide(&p, &permissive(), CompensationCapability::supported(), "h");
        assert!(matches!(d, FinalDecision::Quarantine { .. }));
    }

    #[test]
    fn nan_confidence_fails_closed() {
        // High-stakes: NaN confidence is treated as 0.0, below the bar,
        // and quarantines.
        let p = proposal(ConflictResolution::LastWriteWins, f32::NAN, "b");
        let pol = policy(
            "financial",
            ConflictResolution::LastWriteWins,
            0.5,
            true,
            ConflictResolution::Escalate,
        );
        let d = ConflictResolver::decide(&p, &pol, CompensationCapability::supported(), "h");
        assert!(matches!(d, FinalDecision::Quarantine { .. }));

        // Non-high-stakes: NaN still never auto-applies; it escalates.
        let d =
            ConflictResolver::decide(&p, &permissive(), CompensationCapability::supported(), "h");
        assert!(matches!(d, FinalDecision::Escalate { .. }));

        // Infinite confidence is not a free pass either.
        let p = proposal(ConflictResolution::LastWriteWins, f32::INFINITY, "b");
        let d =
            ConflictResolver::decide(&p, &permissive(), CompensationCapability::supported(), "h");
        assert!(matches!(d, FinalDecision::Escalate { .. }));
    }

    #[test]
    fn decide_is_deterministic() {
        let p = proposal(ConflictResolution::Compensate, 0.9, "b");
        let pol = permissive();
        let cap = CompensationCapability::unsupported();
        let d1 = ConflictResolver::decide(&p, &pol, cap, "h");
        let d2 = ConflictResolver::decide(&p, &pol, cap, "h");
        assert_eq!(d1, d2);
    }

    #[test]
    fn policy_set_exact_beats_default() {
        let set = ConflictPolicySet::new(vec![
            policy(
                "*",
                ConflictResolution::LastWriteWins,
                0.5,
                false,
                ConflictResolution::Escalate,
            ),
            policy(
                "financial",
                ConflictResolution::Escalate,
                0.99,
                true,
                ConflictResolution::Quarantine,
            ),
        ]);
        let fin = set.policy_for("financial");
        assert_eq!(fin.tool_category, "financial");
        assert_eq!(fin.resolution, ConflictResolution::Escalate as i32);
        assert_eq!(fin.auto_approve_threshold, 0.99);
        assert!(fin.require_compensation_support);
    }

    #[test]
    fn policy_set_falls_back_to_org_default_then_builtin() {
        let set = ConflictPolicySet::new(vec![policy(
            "*",
            ConflictResolution::LastWriteWins,
            0.5,
            false,
            ConflictResolution::Escalate,
        )]);
        let fs = set.policy_for("filesystem");
        assert_eq!(fs.tool_category, "filesystem");
        assert_eq!(fs.resolution, ConflictResolution::LastWriteWins as i32);
        assert_eq!(fs.auto_approve_threshold, 0.5);

        let empty = ConflictPolicySet::new(vec![]);
        assert_eq!(empty.policy_for("anything"), {
            let mut d = default_policy();
            d.tool_category = "anything".into();
            d
        });
    }

    #[test]
    fn deny_wins_restrictive_category_policy_beats_permissive_default() {
        let set = ConflictPolicySet::new(vec![
            policy(
                "*",
                ConflictResolution::LastWriteWins,
                0.5,
                false,
                ConflictResolution::Escalate,
            ),
            policy(
                "financial",
                ConflictResolution::Quarantine,
                0.99,
                true,
                ConflictResolution::Quarantine,
            ),
        ]);
        // Merged policy forces QUARANTINE: even a perfect-confidence LWW
        // proposal is vetoed.
        let p = proposal(ConflictResolution::LastWriteWins, 1.0, "b");
        let d = ConflictResolver::decide(
            &p,
            &set.policy_for("financial"),
            CompensationCapability::supported(),
            "h",
        );
        assert!(matches!(d, FinalDecision::Quarantine { .. }));
    }

    #[test]
    fn merge_policies_takes_max_threshold_and_or_compensation() {
        let a = policy(
            "x",
            ConflictResolution::LastWriteWins,
            0.9,
            false,
            ConflictResolution::Escalate,
        );
        let b = policy(
            "x",
            ConflictResolution::Compensate,
            0.4,
            true,
            ConflictResolution::Quarantine,
        );
        let m = merge_policies(&a, &b);
        assert_eq!(m.auto_approve_threshold, 0.9);
        assert!(m.require_compensation_support);
        // COMPENSATE (2) beats LWW (1); QUARANTINE (5) beats ESCALATE (4).
        assert_eq!(m.resolution, ConflictResolution::Compensate as i32);
        assert_eq!(m.fallback, ConflictResolution::Quarantine as i32);
    }
}
