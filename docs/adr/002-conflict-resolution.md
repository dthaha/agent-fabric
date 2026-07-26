# ADR 002: Conflict resolution — offline forks, reconnect merge, four-tier pipeline

- Status: accepted
- Date: 2026-07-26

## Context

Offline-first is the fabric's premise: an endpoint that loses connectivity
keeps working. That means two writers can commit to the same session's
op-log on diverged branches, and on reconnect the fabric must reconcile them.
Every other agent product either can't work offline or silently drops
diverged work. We preserve all of it and *mediate* conflicts. This ADR
freezes the design; Phase A lands the ADR and proto contracts only, with
zero runtime behavior change.

## Decision

### Offline-first premise: forks are a feature

- An offline device runs the brain locally, executes tools, and **commits
  real turns** to its local SQLite op-log. Not previews, not buffers — real
  work. Demoting offline to "preview only" would make us a Copilot clone.
- Offline, the device cannot reach the lease authority, so the lease is
  simply irrelevant: each offline device is **sovereign** — its own writer.
- Local inference is a product requirement, not a UX nicety.

### Lease authority

- **The server is the lease authority.** Leases are granted server-side and
  timestamped with the **server's clock** — never client clocks, which drift
  and are user-settable.
- **Preemption is a presence signal, not a timestamp race.** The lease
  follows where the user is actively working *right now*; the server observes
  "user activity from surface X at server-time T" and grants accordingly.
- The **web client is first-class**: it generates presence (typing in it), so
  it can hold the lease. It can't work offline — which is fine, it's online
  by definition.
- `Lease` gains `granted_by` (server identity) and `preempted_by` (surface
  identity, for audit).

### Reconnect = deterministic merge, then conflict resolution

- On reconnect the device replays its local op-log to the server.
- The server merges **deterministically by `(created_at, entry_id)`** —
  already implemented in `core/context/src/reconcile.rs`. Ordering is free
  and convergent.
- The merge produces a single canonical log; conflict detection then runs
  over the merged region. **The detection unit is the whole turn** (prompt +
  tool calls + response), not an isolated entry.

### Four-tier conflict pipeline

The pipeline deliberately mirrors the existing safety-pipeline shape
(decoder sensor / rules actuator / policy veto) and reuses what already
exists: `reconcile.rs` for the merge, `PolicyGate` (`core/policy`) for the
veto.

**Tier 1 — Structural detector (deterministic, free).** Same tool + same
target + different params → structural conflict. Idempotent (dedupe) and
composable (accept both, ordered) cases are classified without a model.
Runs over every merged region; catches the obvious collisions.

**Tier 2 — Fast conflict classifier (small LLM, classify-only sensor).**
A small LLM (M to single-digit B parameters), **not a DeBERTa encoder**.
Conflict detection is a cold path — it runs on reconnect, not per-prompt —
so the <10ms latency argument for an encoder does not apply; seconds are
fine. A small LLM reads the full conversation context that led to each
action, which an encoder working on isolated pairs cannot. The classifier
catches what the structural detector can't: "book flight" vs "cancel trip"
are different tools, targets, and params — structurally independent — but
semantically opposed. Output is a `ConflictVerdict`: relation
(SUPERSEDES / CONTRADICTS / INDEPENDENT / AMBIGUOUS) + shared entities +
confidence. **Classify-only: the sensor never acts.**

**Tier 3 — Reasoning evaluator / mediator (bigger reasoning-enabled LLM).**
Fires only on what the classifier cannot clear. Reads both full branches
plus tool results, reasons about intent, and either proposes a resolution
(`ResolutionProposal`) or asks a **targeted clarifying question**
(`ClarifyingQuestion`) — the thing a deterministic system literally cannot
do. Output is a proposal, never an action; policy holds the veto. This is
the novelty and the moat: the mediator is post-trained on
diverged-branch → resolution pairs, learning when to resolve silently vs.
ask, what question form resolves fastest, and when actions that *look*
contradictory are actually independent.

**Tier 4 — Policy veto (reuse `PolicyGate` deny-wins).** Maps
verdict/proposal → action via `ConflictPolicy` (per tool-category strategy
wired into the policy gate). Deny-wins. **Fail-closed default =
quarantine.** Per-tool-category confidence thresholds with an org global
default (e.g. financial 0.99, filesystem 0.5): a bank escalates almost
everything, a dev team auto-resolves most — same models, different policy
packs.

### Cascade

```
merged region
  → Tier 1: structural detector (deterministic, free)
    → obvious collision? → policy rule (LWW / compensate / escalate)
    → no collision? → Tier 2: fast classifier (small LLM, M–single-digit B params)
      → high-confidence INDEPENDENT? → auto-approve, done (vast majority of cases)
      → anything else? → Tier 3: reasoning evaluator (bigger LLM, reasoning-enabled)
        → resolves with confidence above policy threshold? → auto-approve
        → below threshold? → escalate / quarantine (rare)
        → can't resolve? → clarifying question → user
  → Tier 4: policy veto on every outcome (deny-wins, fail-closed)
```

### Settled decisions

1. **Compensation contract:** opt-in `compensate()` on the tool trait.
   COMPENSATE/ROLLBACK resolutions are only available when the tool exposes
   compensation; otherwise policy falls back to **ESCALATE**.
2. **Clarifying questions route to the surface holding presence.** If no
   surface is present, queue and surface on next activity.
3. **Quarantine is rare by design.** It happens only when the evaluator
   genuinely can't tell AND policy won't auto-approve. At that point
   blocking is correct — the system is honestly saying "I don't know what
   you meant."
4. **Detection unit is the whole turn** (prompt + tool calls + response).
5. **The classifier is classify-only.** It never acts; high-confidence
   INDEPENDENT skips everything downstream, which is the real speed win.

### Two separate model seams

The classifier and mediator are **separate traits** (`ConflictDecoder`,
`ConflictMediator`) with separate stubs so they can be tested, swapped, and
post-trained independently. They may share a model in v1 (one small LLM, two
prompts: classify → evaluate); in production they are separate,
customer-deployed, and independently post-trainable. Training pipelines:
decoder = entry pairs → labeled relation (synthetic-friendly, small model);
mediator = diverged branches → resolution + clarifying question (harder,
bigger model, the moat).

## Consequences

- New proto contracts in `proto/conflict.proto` (`fabric.conflict`):
  `ConflictRelation`, `ConflictResolution`, `SharedEntity`,
  `ConflictVerdict`, `ClarifyingQuestion`, `ResolutionProposal`,
  `ConflictPolicy`. Contracts only — no logic consumes them yet.
- `Lease` gains optional `granted_by` (field 9) and `preempted_by` (field
  10); existing field numbers are untouched (wire-compatible).
- Cost model: most merges are free (deterministic); most conflicts
  auto-resolve (classifier INDEPENDENT or policy rules); the reasoning
  mediator fires on a small fraction of reconnects, server-side, off the
  hot path.
- Later phases (B–G) build on these frozen contracts: store abstraction,
  server-side lease authority, structural detector, decoder trait, mediator
  trait + mediation flow, reference models.
