//! Runtime plane: the agent loop abstraction, locus-aware handoff protocol,
//! and BYO-agent adapter traits. The runtime is leased together with the
//! context plane — whoever holds the context lease runs the loop this turn.
