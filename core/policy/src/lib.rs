//! Policy plane: dual policy engine. Merges the endpoint (MDM) ceiling with
//! the server additive policy using deny-wins semantics, and gates every
//! tool call, model choice, inference request, data egress, and CUA action.

pub mod eval;
pub mod merge;
pub mod store;

pub use eval::{Decision, DlpOutcome, EvalError, ModelLocus, PolicyGate};
pub use merge::merge;
pub use store::{PolicyStore, StoreError};
