# ADR 008: Harness strategy + server-side agent execution

- Status: accepted
- Date: 2026-08-01

## Context

The fabric needs a reference agent harness — the thing that runs the loop,
calls tools, and talks to models. Building one from scratch is months of work
on a component that is not the product. The product is continuity: leased
context, offline replay, locus-agnostic execution. The harness is a vehicle
for demonstrating that.

Separately, agents must run server-side for long-horizon tasks (the locus
decision matrix: "Long-horizon → server loop, server inference, endpoint
tools via bridge"). The server is not just a database — it's an execution
surface. But it must not become a persistent per-user daemon farm.

## Decision

### 1. Adopt pi (earendil-works/pi) as the harness base

[pi](https://github.com/earendil-works/pi) is MIT-licensed, 81.5k stars,
actively maintained. It provides: agent loop with tool calling, multi-provider
LLM API, compaction/branching, TUI, and — critically — a pluggable
`SessionStore` interface designed for exactly this:

```typescript
interface SessionStore extends AsyncDisposable {
  create(options): Promise<SessionReader>;
  load(metadata): Promise<SessionReader>;
  list(options?): Promise<SessionMetadata[]>;
  appendEntry(metadata, entry: SessionTreeEntry): Promise<void>;
  delete(metadata): Promise<void>;
  fork(source, options, selection): Promise<SessionReader>;
}

interface SessionReader {
  readonly metadata: SessionMetadata;
  readHead(): Promise<{ leafId: string | null }>;
  readEntry(id): Promise<SessionTreeEntry | undefined>;
  readEntries(options?): Promise<SessionTreeEntry[]>;
  readPathToRootOrCompaction(leafId): Promise<SessionTreeEntry[]>;
}
```

We do NOT fork pi. We ship `@fabric/pi-session-backend` — an npm package
implementing `SessionStore` that persists to the Fabric spine. Pi runs
unmodified. This is the standard-proof: Fabric makes an existing popular
agent continuous, rather than being yet another agent stack.

### 2. The session backend is the integration surface

`@fabric/pi-session-backend` maps pi's session model onto Fabric's spine:

| pi concept | Fabric concept |
|---|---|
| `SessionMetadata.id` | `session_id` |
| `SessionTreeEntry` (discriminated union) | `context_entries.payload` + `entry_type` |
| `appendEntry` | `append_entry` (lease-gated, seq-assigned) |
| `readEntries(afterEntrySeq, limit)` | cursor read on `seq` |
| `readPathToRootOrCompaction` | walk `parentId` chain in payload |
| `create` / `delete` / `fork` | session lifecycle ops |

All hard logic (compaction transforms, branch navigation, context building)
lives above the store in pi's `StoreSession` class. We persist entries and
read them back in order. Nothing else.

### 3. Server-side execution = ephemeral K8s Jobs

Server-side agent tasks run as **K8s Jobs** in the customer's cluster — the
same pattern as the terminal tool's ephemeral containers:

```
Terminal tool:  K8s Job → container(org image) → runs shell command → exits
Agent task:     K8s Job → container(agent image) → runs pi agentLoop → exits
```

**Lifecycle:**

1. Delegation arrives (endpoint → server via `HandoffRequest`, or
   admin-initiated)
2. Control plane acquires the write lease for the target session
3. Control plane creates a K8s Job via kube-rs (already integrated)
4. Job spec: org's agent image, resource limits from policy pack, env vars
   (`FABRIC_PG_URL`, `FABRIC_KV_URL`, session ID, lease ID, locus=SERVER)
5. Container starts → `@fabric/pi-session-backend` connects to
   Postgres/Valkey (same cluster network) → loads session context from
   spine → runs headless `agentLoop`
6. Agent appends entries to the spine as it works (visible to endpoint
   via replay)
7. Done / lease revoked / timeout → process exits → Job completes → K8s
   reaps pod
8. Control plane releases lease

**Properties:**

- **Stateless containers.** All state is in Postgres. Kill mid-run,
  restart, it reads the spine and continues.
- **Ephemeral, per-task.** Process lifetime = task lifetime. Not per-user
  daemons.
- **Isolated by K8s.** Namespace policies, resource quotas, network
  policies — the customer's cluster enforces their security posture. The
  fabric creates Pod specs; the cluster admin decides the runtime (gVisor,
  kata, plain runc).
- **Same image pattern as terminal.** The agent container image is the
  org's artifact, bound in their policy pack (`image` + `registry_auth`
  fields, already spec'd).

### 4. Endpoint side = unmodified pi + daemon

On the endpoint, pi runs with its TUI (the interface — no custom GUI needed
beyond a menubar status item). `@fabric/pi-session-backend` talks to the
local Rust daemon over its control socket. The daemon owns lease lifecycle,
offline replay, and conflict resolution. Pi never knows Fabric exists beyond
the store constructor.

### 5. Locus handoff is lease transfer + spine continuity

The protocol already defines this (lease.proto):

- `HandoffRequest` (new_holder_id, reason, locus, ttl_ms)
- `HandoffAck` ("the new holder reports the sequence it has caught up to
  before it begins writing")
- `Locus` enum: ENDPOINT / SERVER / SPLIT

Handoff = transfer write lease + catch-up, NOT summarize-and-restart.
Same session, same seq, different locus.

### 6. Inference taxonomy — what is an agent vs. a gate

| What | How | Where |
|---|---|---|
| Long-horizon user task | pi `agentLoop` (multi-step, tools) | K8s Job, server |
| Conflict decoding | straight inference call (reasoning OFF) | control plane, inline |
| Conflict mediation | straight inference call (reasoning HIGH) | control plane, inline |
| Safety classification | straight inference call | control plane, inline |
| Compaction/summarization | pi internal one-shot | wherever the agent runs |

Only the first row is an "agent." Everything else is a gate or classifier —
single call, structured output, done. These are NOT agent tasks and must
never be implemented as agent loops.

## Consequences

**Positive:**

- No harness to build. Pi is the harness; we own ~6 methods of glue.
- Server execution reuses existing K8s integration. No new infra.
- The demo proves the standard: "your existing agent, now continuous."
- Multi-user isolation is free: one Job per task, one lease per session,
  Postgres row scoping.

**Negative / accepted risks:**

- Dependency on pi's `SessionStore` interface stability. Mitigated: it's
  explicitly designed for pluggable backends, MIT-licensed, and we can
  contribute upstream if it needs widening.
- K8s is a hard server-side dependency. Acceptable: the terminal tool
  already requires it, and the customer owns the cluster.
- Cold start (~2-5s for a node container). Irrelevant for long-horizon
  tasks. Warm pools are the customer's choice.

## Open TODOs

- Evaluate pi's shipped Dockerfile (`node:24-bookworm-slim` +
  `@earendil-works/pi-coding-agent`) as the base agent image vs. NVIDIA
  OpenShell's remote K8s gateway pattern. Both server and endpoint.
- Pi's OpenShell integration has inference routing ("code inside the
  sandbox calls `https://inference.local`, gateway injects credentials
  upstream") which maps to Fabric's admin-configured inference. Evaluate.
