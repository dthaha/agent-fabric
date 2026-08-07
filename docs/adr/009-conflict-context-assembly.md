# ADR 009: Conflict context assembly — shared checkpoint + per-locus tail summaries

- Status: accepted
- Date: 2026-08-06

## Context

When two loci diverge (offline-first endpoints both appending while dark,
ADR 002/006) and their entries collide, the conflict pipeline (ADR 002)
hands context to the decoder and mediator tiers. Today the pipeline feeds
the last N raw entries before the conflict pair (Rust:
`DEFAULT_CONTEXT_WINDOW = 20`).

This has two failure modes that grow with session length:

1. **Prefix blindness.** On a long session the decoder/mediator never sees
   anything before the raw window. The conflict's actual cause often lives
   in the prefix (a goal stated early, a constraint established hundreds of
   turns ago). The model decides blind.
2. **Unbounded in-flight context.** Feeding more raw entries fixes prefix
   blindness only up to the model window; at some depth even a million-
   token window is not enough, and cost scales linearly per conflict.

The spine is already lossless at rest (verbatim entries, never dropped,
quarantine-not-drop). Lossless-at-rest does nothing for the model window —
the problem is what *travels to the model*, not what is stored.

Research basis: Voltropy's LCM paper (Lossless Context Management,
Feb 2026) — dual-state memory (immutable store + derived active context),
engine-driven compaction with coverage metadata, lossless drill-down to
originals. We adopt the architecture, scoped tightly to conflict-time
assembly. We do NOT adopt recursive summarization, summary-of-summary
DAGs, LLM-Map/Agentic-Map, or delegation guards — those belong to the
runtime plane (ADR 008) if ever needed, never the context spine.

### Constraint

Multi-locus by construction. Unlike LCM's single-engine deployment, Fabric
has N endpoints + server, each potentially holding an unsynced tail when a
conflict is detected. Pre-mediation the topology is a genuine fork:

```
shared checkpoint (synced prefix [0..synced_seq])
 ├── tail: endpoint A (unsynced local branch)
 └── tail: endpoint B (unsynced local branch)
      └── raw window: last ~40 turns
           └── conflict pair (always raw, never summarized)
```

Post-mediation branches merge into the single spine and the checkpoint
rolls forward. The divergent window is transient.

## Decision

### Assembly shape

Decoder and mediator receive a bounded, token-budgeted assembly:

```
[checkpoint]   shared summary over converged spine [0..covered_through_seq]
[tails]        one summary per divergent locus, covering that locus's
               unsynced branch since the checkpoint
[raw window]   last ~40 turns, verbatim — where the conflict lives
[conflict pair] the colliding entries, verbatim, never summarized
```

Token budget, not entry count, is the binding constraint. `-40 turns` is a
starting constant for the raw window; turns vary orders of magnitude in
size. Assembly trims from the oldest component forward (checkpoint is
sacred, then tails, then raw window oldest-first).

### Checkpoint is server-computed, never client-computed

LLM summarization is non-deterministic. If two endpoints summarize the same
prefix independently, they produce different digests and the "all parties
share the same checkpoint" invariant breaks. Therefore:

- The checkpoint is computed **once, at reconcile time, over the converged
  spine**, by the server.
- It is distributed to clients via the existing catch-up/handoff path.
- Clients cache it; they never compute or modify it.
- A checkpoint carries coverage metadata — `covered_through_seq` and a
  monotonic version — so roll-forward is deterministic and any party can
  verify the checkpoint covers the prefix it claims.

Checkpoints advance as a chain (v1 → v2 → v3), not a DAG.

### Tail summaries are untrusted claims, ephemeral

Each divergent locus may compute a summary of its own unsynced tail — this
is an optimization for shipping less raw context in a replay. But it is an
endpoint claim, exactly like `created_at` (ADR 006): the server can accept
or recompute per policy. Cheap v1: the server recomputes tail summaries
over the replayed batch at conflict time; client-supplied tail summaries
arrive later.

Tail summaries are mediation-scoped: computed for the conflict, used,
discarded. They are never persisted as authoritative state and never enter
the leased op-log. After mediation the tails merge into the spine and the
checkpoint is the only summary that survives.

### Summaries are derived, not op-log entries

Summary artifacts live in a derived store (seq-range-keyed), never in the
leased op-log. Compaction and assembly are read-path concerns; the spine
stays append-only, single-writer, verbatim. Nothing here changes the
handoff invariant: handoff remains lease-transfer + catch-up, never
summarize-and-restart.

### Structure is a depth-2 tree, not a DAG store

The in-flight assembly is a tree (checkpoint → locus tails → raw window),
depth-fixed at 2, tails sharing exactly one ancestor. It is expressed in
proto as data (`SummaryNode` with seq-range coverage), computed per
mediation. No persistent DAG store, no summary-of-summary recursion.

Drill-down (recovering what a summary covered) is a seq-range fetch against
the spine — the spine's existing addressing IS the lossless pointer layer
LCM built `lcm_expand` for. No graph traversal needed.

A DAG store earns its place only if recursive summaries (3+ levels) or
mediator drill-down loops appear. The coverage-range metadata on
`SummaryNode` is the seam it would grow through.

### Summarizer is a trait with a deterministic default

Summarization is a third inference tier alongside decoder/mediator, behind
the same provider seam. Default implementation is deterministic truncation
(LCM's "level 3" — no LLM, guaranteed convergence); an LLM summarizer plugs
into the same trait. The assembly logic never depends on a model being
available.

## Consequences

- Conflicts on long sessions get shared prefix understanding (checkpoint)
  plus per-locus fidelity exactly where the divergence is (tails + raw
  window). Prefix blindness is gone; in-flight tokens are bounded.
- One new inference tier (summarizer) and one new derived store per
  backend (endpoint SQLite, server Postgres).
- Proto additions: `SummaryNode` message; optional tail summaries on
  `ReplayRequest`; checkpoint on the catch-up/handoff response. The Rust
  tree is frozen — these land in the Go rebase per `docs/port-spec.md`.
- Checkpoint authority is server-only: endpoints cannot forge or fork the
  shared prefix understanding. This is the multi-locus analogue of LCM's
  single-engine property, achieved by trust model instead of topology.
- Tail divergence is resolved by the spine, not by summaries: summaries are
  presentation for the model, the spine remains the source of truth.
