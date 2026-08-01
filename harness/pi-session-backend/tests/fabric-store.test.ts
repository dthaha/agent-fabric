import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer, type Server, type Socket } from "node:net";
import assert from "node:assert/strict";
import test from "node:test";

import { FabricSessionStore, SessionError } from "../src/index.js";
import type { SessionMetadata, SessionTreeEntry } from "../src/index.js";

interface ReceivedRequest {
  id: string;
  method: string;
  params: Record<string, unknown>;
}

type Handler = (request: ReceivedRequest) => unknown;

interface MockDaemon {
  server: Server;
  socketPath: string;
  received: ReceivedRequest[];
  close(): Promise<void>;
}

async function startMockDaemon(handlers: Record<string, Handler>): Promise<MockDaemon> {
  const dir = await mkdtemp(join(tmpdir(), "fabric-store-test-"));
  const socketPath = join(dir, "daemon.sock");
  const received: ReceivedRequest[] = [];
  const sockets = new Set<Socket>();

  const server = createServer((socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    let buffer = "";
    socket.on("data", (chunk: Buffer) => {
      buffer += chunk.toString("utf8");
      for (;;) {
        const newline = buffer.indexOf("\n");
        if (newline === -1) break;
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (line.length === 0) continue;
        const request = JSON.parse(line) as ReceivedRequest;
        received.push(request);
        const handler = handlers[request.method];
        let response: Record<string, unknown>;
        if (!handler) {
          response = {
            id: request.id,
            ok: false,
            error: { code: "unknown", message: `unhandled method ${request.method}` },
          };
        } else {
          try {
            response = { id: request.id, ok: true, result: handler(request) };
          } catch (error) {
            const sessionError = error as SessionError;
            response = {
              id: request.id,
              ok: false,
              error: {
                code: sessionError.code ?? "unknown",
                message: sessionError.message ?? String(error),
              },
            };
          }
        }
        socket.write(`${JSON.stringify(response)}\n`);
      }
    });
  });

  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, () => resolve());
  });

  return {
    server,
    socketPath,
    received,
    async close() {
      for (const socket of sockets) socket.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await rm(dir, { recursive: true, force: true });
    },
  };
}

function makeEntry(id: string, parentId: string | null): SessionTreeEntry {
  return {
    type: "message",
    id,
    parentId,
    timestamp: "2026-08-01T00:00:00.000Z",
    role: "user",
    content: "hello",
  } as SessionTreeEntry;
}

async function withStore(
  handlers: Record<string, Handler>,
  run: (store: FabricSessionStore, daemon: MockDaemon) => Promise<void>,
): Promise<void> {
  const daemon = await startMockDaemon(handlers);
  const store = new FabricSessionStore({ socketPath: daemon.socketPath });
  try {
    await run(store, daemon);
  } finally {
    await store[Symbol.asyncDispose]();
    await daemon.close();
  }
}

test("create sends session.create and returns a reader with metadata", async () => {
  await withStore(
    {
      "session.create": (request) => {
        assert.equal(request.params.id, "fixed-id");
        return { id: "fixed-id", created_at: "2026-08-01T00:00:00.000Z" };
      },
    },
    async (store) => {
      const reader = await store.create({ id: "fixed-id" });
      assert.deepEqual(reader.metadata, {
        id: "fixed-id",
        createdAt: "2026-08-01T00:00:00.000Z",
      });
    },
  );
});

test("load sends session.load and returns a reader", async () => {
  await withStore(
    {
      "session.load": (request) => {
        assert.equal(request.params.id, "s-1");
        return { id: "s-1", created_at: "2026-08-01T00:00:00.000Z" };
      },
    },
    async (store) => {
      const metadata: SessionMetadata = { id: "s-1", createdAt: "" };
      const reader = await store.load(metadata);
      assert.equal(reader.metadata.id, "s-1");
      assert.equal(reader.metadata.createdAt, "2026-08-01T00:00:00.000Z");
    },
  );
});

test("list maps daemon snake_case metadata to pi shape", async () => {
  await withStore(
    {
      "session.list": () => ({
        sessions: [
          { id: "a", created_at: "2026-08-01T00:00:00.000Z" },
          { id: "b", created_at: "2026-08-02T00:00:00.000Z" },
        ],
      }),
    },
    async (store) => {
      const sessions = await store.list();
      assert.deepEqual(sessions, [
        { id: "a", createdAt: "2026-08-01T00:00:00.000Z" },
        { id: "b", createdAt: "2026-08-02T00:00:00.000Z" },
      ]);
    },
  );
});

test("appendEntry sends entry.append with the raw entry payload", async () => {
  const entry = makeEntry("e-1", null);
  await withStore(
    { "entry.append": () => ({}) },
    async (store, daemon) => {
      await store.appendEntry({ id: "s-1", createdAt: "" }, entry);
      const request = daemon.received.find((r) => r.method === "entry.append");
      assert.ok(request);
      assert.equal(request.params.session_id, "s-1");
      assert.deepEqual(request.params.entry, entry);
    },
  );
});

test("delete sends session.delete", async () => {
  await withStore(
    { "session.delete": () => ({}) },
    async (store, daemon) => {
      await store.delete({ id: "s-9", createdAt: "" });
      const request = daemon.received.find((r) => r.method === "session.delete");
      assert.ok(request);
      assert.equal(request.params.id, "s-9");
    },
  );
});

test("fork sends source id, selection, and optional new id", async () => {
  await withStore(
    {
      "session.fork": (request) => {
        assert.equal(request.params.source_id, "src");
        assert.deepEqual(request.params.selection, {
          kind: "through_entry",
          entryId: "e-7",
        });
        assert.equal(request.params.id, "forked");
        return { id: "forked", created_at: "2026-08-03T00:00:00.000Z" };
      },
    },
    async (store) => {
      const reader = await store.fork(
        { id: "src", createdAt: "" },
        { id: "forked" },
        { kind: "through_entry", entryId: "e-7" },
      );
      assert.equal(reader.metadata.id, "forked");
      assert.equal(reader.metadata.createdAt, "2026-08-03T00:00:00.000Z");
    },
  );
});

test("reader.readHead maps leaf_id to leafId", async () => {
  await withStore(
    {
      "session.create": () => ({ id: "s-1", created_at: "2026-08-01T00:00:00.000Z" }),
      "session.head": (request) => {
        assert.equal(request.params.session_id, "s-1");
        return { leaf_id: "e-3" };
      },
    },
    async (store) => {
      const reader = await store.create({});
      assert.deepEqual(await reader.readHead(), { leafId: "e-3" });
    },
  );
});

test("reader.readEntry returns undefined when the daemon returns null", async () => {
  await withStore(
    {
      "session.create": () => ({ id: "s-1", created_at: "2026-08-01T00:00:00.000Z" }),
      "entry.read": (request) => {
        assert.equal(request.params.id, "nope");
        return { entry: null };
      },
    },
    async (store) => {
      const reader = await store.create({});
      assert.equal(await reader.readEntry("nope"), undefined);
    },
  );
});

test("reader.readEntries passes cursor options as after_seq and limit", async () => {
  await withStore(
    {
      "session.create": () => ({ id: "s-1", created_at: "2026-08-01T00:00:00.000Z" }),
      "entry.list": (request) => {
        assert.equal(request.params.session_id, "s-1");
        assert.equal(request.params.after_seq, 41);
        assert.equal(request.params.limit, 10);
        return { entries: [makeEntry("e-42", "e-41")] };
      },
    },
    async (store) => {
      const reader = await store.create({});
      const entries = await reader.readEntries({ afterEntrySeq: 41, limit: 10 });
      assert.equal(entries.length, 1);
      assert.equal(entries[0]?.id, "e-42");
    },
  );
});

test("reader.readEntries omits cursor params when options are absent", async () => {
  await withStore(
    {
      "session.create": () => ({ id: "s-1", created_at: "2026-08-01T00:00:00.000Z" }),
      "entry.list": (request) => {
        assert.equal("after_seq" in request.params, false);
        assert.equal("limit" in request.params, false);
        return { entries: [] };
      },
    },
    async (store) => {
      const reader = await store.create({});
      assert.deepEqual(await reader.readEntries(), []);
    },
  );
});

test("reader.readPathToRootOrCompaction delegates the walk to the daemon", async () => {
  await withStore(
    {
      "session.create": () => ({ id: "s-1", created_at: "2026-08-01T00:00:00.000Z" }),
      "entry.path": (request) => {
        assert.equal(request.params.session_id, "s-1");
        assert.equal(request.params.leaf_id, "e-3");
        return {
          entries: [makeEntry("e-3", "e-2"), makeEntry("e-2", "e-1"), makeEntry("e-1", null)],
        };
      },
    },
    async (store) => {
      const reader = await store.create({});
      const path = await reader.readPathToRootOrCompaction("e-3");
      assert.deepEqual(
        path.map((entry) => entry.id),
        ["e-3", "e-2", "e-1"],
      );
    },
  );
});

test("daemon errors propagate as SessionError with matching code", async () => {
  await withStore(
    {
      "session.load": () => {
        throw new SessionError("not_found", "no such session");
      },
    },
    async (store) => {
      await assert.rejects(
        () => store.load({ id: "missing", createdAt: "" }),
        (error: unknown) => {
          assert.ok(error instanceof SessionError);
          assert.equal(error.code, "not_found");
          assert.equal(error.message, "no such session");
          return true;
        },
      );
    },
  );
});

test("store defaults to FABRIC_SOCKET_PATH env var", async () => {
  const daemon = await startMockDaemon({ "session.list": () => ({ sessions: [] }) });
  const previous = process.env.FABRIC_SOCKET_PATH;
  process.env.FABRIC_SOCKET_PATH = daemon.socketPath;
  try {
    const store = new FabricSessionStore();
    try {
      assert.deepEqual(await store.list(), []);
    } finally {
      await store[Symbol.asyncDispose]();
    }
  } finally {
    if (previous === undefined) {
      delete process.env.FABRIC_SOCKET_PATH;
    } else {
      process.env.FABRIC_SOCKET_PATH = previous;
    }
    await daemon.close();
  }
});
