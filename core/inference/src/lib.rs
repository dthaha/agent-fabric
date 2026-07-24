//! Inference plane: swappable hosted inference clients (OpenAI-compatible,
//! Bedrock, Foundry) behind a common trait. Inference is a commodity — the
//! admin configures providers, the fabric routes by policy and locus.
