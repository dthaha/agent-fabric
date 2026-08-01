# @fabric/pi-session-backend

Pi `SessionStore` implementation that persists session trees to the Agent
Fabric spine via the local endpoint daemon's control socket.

The package is a **thin client**: it serializes pi session operations into
NDJSON requests over a Unix domain socket. The daemon owns lease gating,
sequence assignment, offline buffering, and conflict resolution. See
`docs/adr/008-harness-and-server-execution.md`.

## Usage

```typescript
import { FabricSessionStore } from "@fabric/pi-session-backend";

await using store = new FabricSessionStore(); // FABRIC_SOCKET_PATH or /tmp/fabric-endpoint.sock
const session = await store.create({});
await store.appendEntry(session.metadata, entry);
const head = await session.readHead();
```

## Wire protocol

NDJSON over the daemon control socket — one JSON object per line. Requests
carry `{ id, method, params }`; responses carry `{ id, ok, result | error }`.
Daemon error codes map to pi's `SessionErrorCode`.

Methods: `session.create`, `session.load`, `session.list`, `session.delete`,
`session.fork`, `session.head`, `entry.append`, `entry.read`, `entry.list`,
`entry.path`.

## Development

```bash
npm run typecheck   # tsc --noEmit
npm test            # node --test against a mock socket daemon (no real daemon needed)
```

Zero runtime dependencies. Node.js >= 20. Peer: `@earendil-works/pi-agent-core`.
Apache-2.0.
