# Conflict Resolution — Plan

**Date:** 2026-07-26
**Status:** Planning (not yet handed to OpenCode)
**Why this is novel:** Offline-first means forks are a *feature*, not a failure. Every
other agent product (Claude Cowork, etc.) either can't work offline or silently drops
diverged work. The fabric preserves all of it and *mediates* conflicts with a
context-aware model that can ask the user a targeted clarifying question. That mediation
layer — deterministic detection + post-trained reasoning model + policy veto — is the
differentiator.

---

## The model (settled in design discussion)

### Lease authority
- **Server is the lease authority.** Leases are granted server-side, timestamped with the
  *server's* clock (never the client's — device clocks drift and are user-settable).
- **Preemption is a presence signal, not a timestamp race.** The lease follows where the
  user is actively working *right now*. Server observes "user activity from surface X at
  server-time T" and grants accordingly.
- **Web client is first-class.** It generates presence (typing in it), so it can hold the
  lease. It just can't work offline — which is fine, it's online by definition.

### Offline = sovereign, real commits
- An offline device runs the brain locally, executes tools, and **commits real turns** to
  its local SQLite op-log. Not previews. Not buffers. Real work. (This is the whole point —
  demoting offline to "preview only" makes us Claude Cowork.)
- Offline, the device can't reach the server to hold a lease, so the lease is simply
  irrelevant offline. Each offline device is its own writer.
- Local inference is a **product requirement**, not a UX nicety. Any far-end client must be
  capable of local inference to participate.

### Reconnect = merge, then mediate
- Device replays its local op-log to the server.
- Server merges deterministically by `(created_at, entry_id)` — **already implemented** in
  `reconcile.rs`. Ordering is free and convergent.
- Merge produces a single canonical log. Then conflict detection runs over the merged
  region.

### Conflict taxonomy
| Type | Example | Detection | Resolution |
|---|---|---|---|
| Idempotent | both queried same API | deterministic | dedupe |
| Composable | one wrote file, other sent email | deterministic | accept both, ordered |
| Structural | both `set_config(key=X)`, diff values | deterministic (same tool+target) | policy rule (LWW/compensate) |
| Semantic | "book flight" vs "cancel trip" | **decoder model** | **mediator model** |

The first three never need a model. Semantic conflicts need two: a decoder to detect, a
mediator to resolve.

### Four-tier conflict pipeline (mirrors the safety pipeline shape)

**Tier 1 — Structural detector (deterministic, free).**
Same tool + same target + different params → structural conflict. Idempotent/composable
classified without a model. Runs over every merged region. Catches the obvious collisions.

**Tier 2 — Semantic conflict decoder (model, sensor).**
The Granite Guardian equivalent. A dedicated classifier that reads merged entries and answers:
"do these conflict, and what kind?" Output is a `ConflictVerdict`:
- `ConflictRelation`: SUPERSEDES / CONTRADICTS / INDEPENDENT / AMBIGUOUS
- Shared entities and intent overlap
- Confidence score

This catches what the structural detector *can't*: "book flight" vs "cancel trip" are
different tools, different targets, different params — structurally independent. Only a model
sees they're about the same underlying intent and they oppose each other.

Model profile: off-the-shelf or lightly fine-tuned classifier. DeBERTa-class, like the
RouteLLM load classifier. Cheap, fast, always-on over the merged region. Customer-deployed
(same posture as safety decoder — fabric ships the trait + proto, customer runs the model).

Training data: pairs of entries → labeled relation. Easier to generate synthetically than
mediator data. Smaller model.

**Tier 3 — Conflict mediator (post-trained reasoning model).**
The novel part. Invoked *only* when the decoder flags a real semantic conflict that policy
can't auto-resolve (below threshold, or resolution = ESCALATE). Reads full conversation
history on both branches + tool results, reasons about intent, and either:
- resolves with high confidence ("cancel clearly supersedes — user said 'never mind'"), or
- asks a **targeted clarifying question** ("did the cancel supersede the booking, or were
  these different parts of the trip?") — the thing a deterministic system literally cannot do.

Output is a structured resolution proposal, never an action. Policy holds the veto.

Model profile: post-trained reasoning model, fine-tuned on diverged-branch → verdict pairs.
Bigger than the decoder. The moat — "we post-train a conflict mediator" is harder to copy
than "we prompt GPT." Learns: when to resolve silently vs. ask, what kind of question
resolves fastest (yes/no > multiple choice > open-ended), how to read conversation history
for intent signals, when two actions that *look* contradictory are actually independent.

Runs server-side during reconciliation. Not on the endpoint. Not on the hot path. Only when
a conflict actually needs mediation.

**Tier 4 — Policy veto (reuse existing `PolicyGate` pattern).**
Maps decoder verdict + mediator proposal → action. Deny-wins. Per-org confidence thresholds.
Fail-closed default = quarantine.

### Why the decoder/mediator split matters
Without the decoder, you'd invoke the expensive reasoning model on every merged region to
ask "is there even a conflict here?" With it, the mediator fires on maybe 5% of reconnects —
only the ones the decoder flags as genuinely ambiguous semantic conflicts. The decoder is
what makes the mediator *cheap*.

This also gives two clean training pipelines:
- **Decoder:** entry pairs → labeled relation. Synthetic-friendly. Small model.
- **Mediator:** diverged branches → resolution + clarifying question. Harder to generate.
  Bigger model. The moat.

### Policy veto (reuse existing pattern)
- Reuse `PolicyGate`'s deny-wins shape. Map `ConflictVerdict` → action, veto wins.
- Per-org confidence thresholds: a bank sets 0.99 (almost everything escalates), a dev team
  0.7 (auto-resolve most). Same model, different policy packs.
- **Fail-closed default = quarantine.** No rule / below threshold / unresolvable → the
  conflicting turns are quarantined (brain doesn't build on them) until resolved. Mirrors
  the existing `empty lease_id → PolicyDenied` posture.

### Cost model
Most merges are free (deterministic). Most conflicts auto-resolve (policy rules). The model
fires only on genuinely ambiguous semantic conflicts — a small fraction of reconnects. Off
the hot path, server-side, only when needed.

---

## What already exists (do NOT rebuild)
- `core/context/src/reconcile.rs` — deterministic `(created_at, entry_id)` merge, seq-collision
  handling, `ReconcileReport`. **The ordering layer is done.**
- `core/policy/src/eval.rs` — `PolicyGate`, `Decision` (Allow/Deny/RequireApproval),
  deny-wins, `DlpAction` (Block/Redact/LogOnly). **The veto pattern is done.**
- `proto/context.proto`, `proto/policy.proto`, `proto/lease.proto` — wire contracts.

## What's new
1. **Lease authority refactor** — move lease grant/renew/preempt from local SQLite to a
   server API; endpoint caches a server-granted lease token; SQLite becomes the offline
   op-log (real commits), not the lease source.
2. **Structural conflict detector** — deterministic (same tool + same target + different
   params) over the merged region. Cheap, no model. Tier 1.
3. **`ConflictDecoder` trait + `ConflictVerdict` proto** — the semantic sensor seam.
   Classifies relation (SUPERSEDES/CONTRADICTS/INDEPENDENT/AMBIGUOUS) + shared entities +
   confidence. DeBERTa-class, customer-deployed. Tier 2. (Granite Guardian equivalent.)
4. **`ConflictMediator` trait + resolution-proposal proto** — the post-trained reasoning
   seam. Reads full branch context, proposes a resolution or a clarifying question.
   Customer-deployed. Tier 3. (The novelty / moat.)
5. **Conflict policy** — `ConflictPolicy` proto (per tool-category resolution strategy +
   confidence threshold + fallback), wired into `PolicyGate`. Tier 4.
6. **Mediation flow** — structural detector → decoder → policy auto-resolve where possible →
   mediator for the rest → policy veto on proposal → execute resolution (LWW / compensate /
   escalate / quarantine / rollback) or surface a clarifying question to the user.
7. **`ContextStore` trait** — abstract the SQLite store so server-side can use Postgres
   (multi-replica / HA) while endpoint keeps SQLite. (Pays down the "SQLite on both sides"
   debt.)

---

## Phases

### Phase A — ADR + proto contracts (no behavior change)
- Write `docs/adr/00X-conflict-resolution.md` capturing: lease authority model, offline
  sovereign commits, fork-merge, conflict taxonomy, **four-tier pipeline (structural
  detector / decoder / mediator / policy veto)**, fail-closed.
- Add proto:
  - `ConflictVerdict` + `ConflictRelation` (SUPERSEDES/CONTRADICTS/INDEPENDENT/AMBIGUOUS)
    + shared entities + confidence — **decoder output**.
  - `ResolutionProposal` + `ClarifyingQuestion` — **mediator output**.
  - `ConflictPolicy` + `ConflictResolution` enum
    (LAST_WRITE_WINS/COMPENSATE/ESCALATE/QUARANTINE/ROLLBACK).
- Extend `Lease` proto: `granted_by` (server identity), `preempted_by` (audit).
- `make proto` regen. **No logic yet — contracts only.**
- *Exit:* buf lint clean, workspace compiles, ADR merged.

### Phase B — `ContextStore` trait + SQLite backend
- Extract the concrete `rusqlite` store in `db.rs` behind an async `ContextStore` trait
  (append, entries_since, lease_*, insert_entry_raw, entry_at_seq, entry_by_id).
- `SqliteContextStore` = current behavior wrapped in `spawn_blocking`.
- `reconcile.rs` + `handoff.rs` re-targeted at the trait.
- *Exit:* all existing context tests pass unchanged against the trait. No behavior change.
- *Note:* Postgres backend is a later phase — trait first, second backend when HA lands.

### Phase C — Server-side lease authority
- New lease API on the control plane: `POST /lease/acquire`, `/preempt`, `/renew`,
  `DELETE /lease/release`. Server stamps with its own clock.
- Endpoint daemon: request lease on user activity, cache token + expiry, buffer offline,
  replay + re-request on reconnect.
- Preemption = presence event. Latest server-observed activity wins.
- *Exit:* lease handoff + offline reconcile tests still mandatory and passing (per AGENTS.md
  non-negotiable #3). New test: two devices + server, presence-driven preemption.

### Phase D — Structural conflict detector (Tier 1)
- Detector over the merged region: same tool + same target + different params → structural
  conflict. Idempotent/composable classified without a model.
- Extend `ReconcileReport` with a `conflicts: Vec<DetectedConflict>` (structural only here).
- *Exit:* unit tests for idempotent dedupe, composable accept-both, structural flag.

### Phase E — `ConflictDecoder` trait (Tier 2, the sensor)
- `ConflictDecoder` trait: `classify(entries) -> Vec<ConflictVerdict>` (async, model-agnostic).
  Mirrors the safety decoder / `ModelClassifier` seam.
- Stub decoder (returns AMBIGUOUS for everything) for tests; real model is customer-deployed.
- Wire decoder after the structural detector: structural catches the obvious, decoder catches
  semantic conflicts the structural detector can't see (different tools, same intent).
- *Exit:* integration tests — decoder flags a "book vs cancel" semantic conflict the
  structural detector missed; INDEPENDENT pairs pass through untouched. Deterministic given
  a stub decoder.

### Phase F — `ConflictMediator` trait + mediation flow (Tier 3 + 4)
- `ConflictMediator` trait: `mediate(conflict, branch_context) -> ResolutionProposal`
  (async, model-agnostic). The post-trained reasoning seam.
- Stub mediator (returns "escalate + generic clarifying question") for tests.
- Mediation flow: structural detector → decoder → policy auto-resolve where possible →
  mediator for the rest → policy veto on proposal → execute resolution or surface the
  clarifying question.
- Wire `ConflictPolicy` into `PolicyGate` (per tool-category strategy + threshold + fallback).
- *Exit:* integration tests — LWW path, compensate path, escalate path, quarantine
  fail-closed, clarifying-question path. All deterministic given stub decoder + mediator.

### Phase G — Reference models + docs
- Reference `ConflictDecoder` impl (prompted off-the-shelf classifier, like the Bonsai 1.7B
  stopgap) and reference `ConflictMediator` impl (prompted reasoning model) so the feature is
  demoable end-to-end without post-trained models.
- Document both post-training data shapes:
  - Decoder: entry pairs → labeled relation (synthetic-friendly, small model).
  - Mediator: diverged branches → resolution + clarifying question (the moat).
- README + ADR cross-links. Call out the mediator post-training as the differentiator.

### Out of scope (explicitly)
- Postgres `ContextStore` backend (Phase B sets up the trait; second backend when HA lands).
- The actual post-trained decoder + mediator models (customer artifacts; we ship the seams +
  reference impls).
- CUA / tool-plane changes (Phase 6, separate).
- Multi-region HA failover (enterprise/ha, later).

---

## Decisions (settled 2026-07-26)

1. **Compensation contract:** opt-in `compensate()` on the tool trait. Tools that mutate state
   implement it; tools that don't return `None`. COMPENSATE/ROLLBACK resolutions only available
   when the tool exposes compensation; otherwise policy falls back to ESCALATE. Additive —
   existing Phase 6 tools just return `None` until they implement it.

2. **Clarifying-question surface:** route to whichever surface currently holds presence (the
   lease signal). If no surface is present, queue and surface on next activity.

3. **Quarantine / auto-approve cascade:** the fast classifier is the gatekeeper. Most merged
   regions get a high-confidence "no conflict" and pass through untouched — no quarantine, no
   mediator, no user interruption. The reasoning evaluator only fires on the slice the
   classifier can't clear. Policy then decides whether the evaluator's verdict auto-approves
   (above threshold) or escalates/quarantines (below threshold). Quarantine is rare by design:
   it only happens when the evaluator genuinely can't tell AND policy says don't auto-approve.
   At that point, blocking the user is correct — the system is honestly saying "I don't know
   what you meant."

4. **Threshold / detection unit:** the unit of conflict detection is the whole turn
   (prompt + tool calls + response), not an isolated entry. Per-tool-category thresholds with
   a global org default (e.g. financial 0.99, filesystem 0.5, fallback to org global).

5. **Decoder behavior:** classify-only. The sensor never acts — keeps the safety-pipeline
   symmetry and the audit story clean. Speed optimization (skip mediator on high-confidence
   SUPERSEDES) deferred; the classifier's high-confidence INDEPENDENT already skips everything
   downstream, which is the real speed win.

### Model cascade (settled)

The decoder and mediator are **separate traits** but may share a model in v1. The cascade:

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

The fast classifier is a **small LLM** (Bonsai-class, M to single-digit B params), not a
DeBERTa encoder. Conflict detection is a cold path (runs on reconnect, not per-prompt), so
the <10ms latency argument for an encoder doesn't apply. A small LLM reads the full
conversation context that led to each action — an encoder working on isolated pairs is
gimped by comparison. Seconds are fine on a cold path.

The reasoning evaluator needs to be fast *enough* (it's still a reconnect path, not live
chat), but nowhere near as fast as the classifier. It's the deeper pass — reads both full
branches, reasons about intent, proposes a resolution or asks a targeted clarifying question.

v1 reference impl: one small LLM, two prompts (classify → evaluate). Production: separate
models, both customer-deployed, both post-trainable. The mediator post-training is the moat.

---

## Sequencing note
A → B → C can proceed somewhat independently (contracts, store abstraction, lease authority).
D depends on B (needs the reconciled merged region). E depends on D + the proto from A.
F depends on E. G depends on F. **Recommend landing A first** (it's just contracts + the ADR,
zero risk) so the design is frozen on `main` before any behavior changes.

The two model seams (decoder in E, mediator in F) are deliberately separate traits with
separate stubs so they can be tested, swapped, and post-trained independently. The decoder is
the cheap always-on sensor; the mediator is the expensive reasoning model that only fires when
the decoder + policy say it must.
