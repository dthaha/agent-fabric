use crate::safety::{
    parse_safety_category, SafetyAction, SafetyCategory, SafetyEnforcement, SafetyVerdict,
};
use fabric_types::policy::{SafetyAction as ProtoSafetyAction, SafetyPolicyRule};

pub struct SafetyPolicyEnforcer {
    rules: Vec<PolicyRule>,
    default_action: SafetyAction,
}

struct PolicyRule {
    category: SafetyCategory,
    action: SafetyAction,
}

fn map_proto_action(action: i32) -> SafetyAction {
    match ProtoSafetyAction::try_from(action) {
        Ok(ProtoSafetyAction::Block) => SafetyAction::Block,
        Ok(ProtoSafetyAction::ForceEndpoint) => SafetyAction::ForceEndpoint,
        Ok(ProtoSafetyAction::Warn) => SafetyAction::Warn,
        _ => SafetyAction::Allow,
    }
}

fn parse_category(s: &str) -> SafetyCategory {
    parse_safety_category(s)
}

impl SafetyPolicyEnforcer {
    pub fn new(rules: Vec<SafetyPolicyRule>, default_action: SafetyAction) -> Self {
        Self {
            rules: rules
                .into_iter()
                .map(|r| PolicyRule {
                    category: parse_category(&r.category),
                    action: map_proto_action(r.action),
                })
                .collect(),
            default_action,
        }
    }

    pub fn enforce(&self, verdict: &SafetyVerdict) -> SafetyEnforcement {
        if verdict.verdict == crate::safety::SafetyLevel::Safe {
            return SafetyEnforcement {
                action: SafetyAction::Allow,
                triggered_categories: Vec::new(),
                blocked: false,
                force_endpoint: false,
            };
        }

        let mut strictest: SafetyAction = self.default_action.clone();
        let mut triggered = Vec::new();

        for detected in &verdict.categories {
            let action = self
                .rules
                .iter()
                .find(|r| r.category == *detected)
                .map(|r| r.action.clone())
                .unwrap_or_else(|| self.default_action.clone());

            triggered.push(detected.clone());

            if is_stricter(&action, &strictest) {
                strictest = action;
            }
        }

        let blocked = strictest == SafetyAction::Block;
        let force_endpoint = strictest == SafetyAction::ForceEndpoint;

        SafetyEnforcement {
            action: strictest,
            triggered_categories: triggered,
            blocked,
            force_endpoint,
        }
    }
}

fn is_stricter(a: &SafetyAction, b: &SafetyAction) -> bool {
    fn rank(action: &SafetyAction) -> u8 {
        match action {
            SafetyAction::Block => 4,
            SafetyAction::ForceEndpoint => 3,
            SafetyAction::Warn => 2,
            SafetyAction::Allow => 1,
        }
    }
    rank(a) > rank(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::SafetyLevel;
    use fabric_types::policy::SafetyAction as ProtoSafetyAction;

    fn block_rule(category: &str) -> SafetyPolicyRule {
        SafetyPolicyRule {
            category: category.into(),
            action: ProtoSafetyAction::Block as i32,
        }
    }

    fn warn_rule(category: &str) -> SafetyPolicyRule {
        SafetyPolicyRule {
            category: category.into(),
            action: ProtoSafetyAction::Warn as i32,
        }
    }

    fn force_rule(category: &str) -> SafetyPolicyRule {
        SafetyPolicyRule {
            category: category.into(),
            action: ProtoSafetyAction::ForceEndpoint as i32,
        }
    }

    fn verdict(level: SafetyLevel, cats: Vec<SafetyCategory>) -> SafetyVerdict {
        SafetyVerdict {
            verdict: level,
            categories: cats,
            explanation: None,
            model_id: "test".into(),
            raw_output: "".into(),
        }
    }

    #[test]
    fn safe_verdict_allows() {
        let enforcer = SafetyPolicyEnforcer::new(vec![block_rule("violence")], SafetyAction::Allow);
        let v = verdict(SafetyLevel::Safe, vec![]);
        let e = enforcer.enforce(&v);
        assert!(!e.blocked);
        assert!(!e.force_endpoint);
        assert_eq!(e.action, SafetyAction::Allow);
    }

    #[test]
    fn block_detected_category() {
        let enforcer = SafetyPolicyEnforcer::new(vec![block_rule("violence")], SafetyAction::Allow);
        let v = verdict(SafetyLevel::Unsafe, vec![SafetyCategory::Violence]);
        let e = enforcer.enforce(&v);
        assert!(e.blocked);
        assert!(!e.force_endpoint);
        assert_eq!(e.action, SafetyAction::Block);
    }

    #[test]
    fn force_endpoint_detected_category() {
        let enforcer = SafetyPolicyEnforcer::new(vec![force_rule("pii")], SafetyAction::Allow);
        let v = verdict(SafetyLevel::Unsafe, vec![SafetyCategory::Pii]);
        let e = enforcer.enforce(&v);
        assert!(!e.blocked);
        assert!(e.force_endpoint);
        assert_eq!(e.action, SafetyAction::ForceEndpoint);
    }

    #[test]
    fn deny_wins_over_warn() {
        let enforcer = SafetyPolicyEnforcer::new(
            vec![block_rule("violence"), warn_rule("profanity")],
            SafetyAction::Allow,
        );
        let v = verdict(
            SafetyLevel::Unsafe,
            vec![SafetyCategory::Violence, SafetyCategory::Profanity],
        );
        let e = enforcer.enforce(&v);
        assert!(e.blocked);
        assert_eq!(e.action, SafetyAction::Block);
    }

    #[test]
    fn force_endpoint_wins_over_warn() {
        let enforcer = SafetyPolicyEnforcer::new(
            vec![force_rule("pii"), warn_rule("profanity")],
            SafetyAction::Allow,
        );
        let v = verdict(
            SafetyLevel::Unsafe,
            vec![SafetyCategory::Pii, SafetyCategory::Profanity],
        );
        let e = enforcer.enforce(&v);
        assert!(!e.blocked);
        assert!(e.force_endpoint);
        assert_eq!(e.action, SafetyAction::ForceEndpoint);
    }

    #[test]
    fn default_action_for_unmatched_category() {
        let enforcer = SafetyPolicyEnforcer::new(vec![block_rule("violence")], SafetyAction::Warn);
        let v = verdict(SafetyLevel::Unsafe, vec![SafetyCategory::Injection]);
        let e = enforcer.enforce(&v);
        assert!(!e.blocked);
        assert!(!e.force_endpoint);
        assert_eq!(e.action, SafetyAction::Warn);
    }

    #[test]
    fn custom_category_matches() {
        let enforcer = SafetyPolicyEnforcer::new(
            vec![SafetyPolicyRule {
                category: "custom_risk".into(),
                action: ProtoSafetyAction::Block as i32,
            }],
            SafetyAction::Allow,
        );
        let v = verdict(
            SafetyLevel::Unsafe,
            vec![SafetyCategory::Custom("custom_risk".into())],
        );
        let e = enforcer.enforce(&v);
        assert!(e.blocked);
    }

    #[test]
    fn parser_alias_matches_policy_rule() {
        // Regression: a parser-emitted alias ("Hate Speech" → Profanity)
        // must match the same category in a policy rule ("profanity").
        let enforcer =
            SafetyPolicyEnforcer::new(vec![block_rule("profanity")], SafetyAction::Allow);
        let v = verdict(
            SafetyLevel::Unsafe,
            vec![parse_safety_category("Hate Speech")],
        );
        assert!(enforcer.enforce(&v).blocked);
    }

    #[test]
    fn triggered_categories_propagated() {
        let enforcer = SafetyPolicyEnforcer::new(vec![block_rule("violence")], SafetyAction::Allow);
        let v = verdict(
            SafetyLevel::Unsafe,
            vec![SafetyCategory::Violence, SafetyCategory::Injection],
        );
        let e = enforcer.enforce(&v);
        assert_eq!(e.triggered_categories.len(), 2);
        assert!(e.triggered_categories.contains(&SafetyCategory::Violence));
        assert!(e.triggered_categories.contains(&SafetyCategory::Injection));
    }

    #[test]
    fn empty_rules_uses_default() {
        let enforcer = SafetyPolicyEnforcer::new(vec![], SafetyAction::Block);
        let v = verdict(SafetyLevel::Unsafe, vec![SafetyCategory::Violence]);
        let e = enforcer.enforce(&v);
        assert!(e.blocked);
        assert_eq!(e.action, SafetyAction::Block);
    }
}
