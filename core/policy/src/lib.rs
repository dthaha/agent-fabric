//! Policy plane: dual policy engine. Merges the endpoint (MDM) ceiling with
//! the server additive policy using deny-wins semantics, and gates every
//! tool call, model choice, inference request, data egress, and CUA action.

pub mod conflict;
pub mod eval;
pub mod merge;
pub mod store;

pub use conflict::{
    default_policy, is_high_stakes, merge_policies, CompensationCapability, ConflictPolicySet,
    ConflictResolver, FinalDecision, DEFAULT_AUTO_APPROVE_THRESHOLD, HIGH_STAKES_THRESHOLD,
};
pub use eval::{Decision, DlpOutcome, EvalError, ModelLocus, PolicyGate};
pub use merge::merge;
pub use store::{PolicyStore, StoreError};
