import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer, type Server, type Socket } from "node:net";
import assert from "node:assert/strict";
import test from "node:test";

import { SessionError } from "../src/index.js";
import { Transport } from "../src/transport.js";

interface ReceivedRequest {
  id: string;
  method: string;
  params: Record<string, unknown>;
}

interface MockDaemon {
  server: Server;
  socketPath: string;
  received: ReceivedRequest[];
  respond: (request: ReceivedRequest, socket: Socket) => void;
  close(): Promise<void>;
}

async function startMockDaemon(
  respond: (request: ReceivedRequest, socket: Socket) => void,
): Promise<MockDaemon> {
  const dir = await mkdtemp(join(tmpdir(), "fabric-transport-test-"));
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
        respond(request, socket);
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
    respond,
    async close() {
      for (const socket of sockets) socket.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await rm(dir, { recursive: true, force: true });
    },
  };
}

function ok(socket: Socket, id: string, result: unknown): void {
  socket.write(`${JSON.stringify({ id, ok: true, result })}\n`);
}

test("request/response round trip over unix socket", async (t) => {
  const daemon = await startMockDaemon((request, socket) => {
    ok(socket, request.id, { echoed: request.method });
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  t.after(() => transport[Symbol.asyncDispose]());

  const result = await transport.request<{ echoed: string }>("session.list", {});
  assert.deepEqual(result, { echoed: "session.list" });
  assert.equal(daemon.received.length, 1);
  assert.equal(daemon.received[0]?.method, "session.list");
  assert.deepEqual(daemon.received[0]?.params, {});
});

test("concurrent requests match responses by id", async (t) => {
  const daemon = await startMockDaemon((request, socket) => {
    const delay = request.method === "slow" ? 50 : 0;
    setTimeout(() => ok(socket, request.id, { method: request.method }), delay);
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  t.after(() => transport[Symbol.asyncDispose]());

  const [slow, fast] = await Promise.all([
    transport.request<{ method: string }>("slow", {}),
    transport.request<{ method: string }>("fast", {}),
  ]);
  assert.equal(slow.method, "slow");
  assert.equal(fast.method, "fast");
});

test("daemon error responses map to SessionError with matching code", async (t) => {
  const daemon = await startMockDaemon((request, socket) => {
    socket.write(
      `${JSON.stringify({
        id: request.id,
        ok: false,
        error: { code: "not_found", message: "no such session" },
      })}\n`,
    );
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  t.after(() => transport[Symbol.asyncDispose]());

  await assert.rejects(
    () => transport.request("session.load", { id: "missing" }),
    (error: unknown) => {
      assert.ok(error instanceof SessionError);
      assert.equal(error.code, "not_found");
      assert.equal(error.message, "no such session");
      return true;
    },
  );
});

test("unknown daemon error codes map to unknown", async (t) => {
  const daemon = await startMockDaemon((request, socket) => {
    socket.write(
      `${JSON.stringify({
        id: request.id,
        ok: false,
        error: { code: "weird_daemon_thing", message: "boom" },
      })}\n`,
    );
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  t.after(() => transport[Symbol.asyncDispose]());

  await assert.rejects(
    () => transport.request("session.list", {}),
    (error: unknown) => {
      assert.ok(error instanceof SessionError);
      assert.equal(error.code, "unknown");
      return true;
    },
  );
});

test("connection refused surfaces as storage SessionError", async () => {
  const dir = await mkdtemp(join(tmpdir(), "fabric-transport-test-"));
  const transport = new Transport({ socketPath: join(dir, "nothing-here.sock") });

  await assert.rejects(
    () => transport.request("session.list", {}),
    (error: unknown) => {
      assert.ok(error instanceof SessionError);
      assert.equal(error.code, "storage");
      return true;
    },
  );

  await transport[Symbol.asyncDispose]();
  await rm(dir, { recursive: true, force: true });
});

test("partial reads are framed correctly across chunk boundaries", async (t) => {
  const daemon = await startMockDaemon((request, socket) => {
    const payload = `${JSON.stringify({ id: request.id, ok: true, result: { big: "x".repeat(4096) } })}\n`;
    const half = Math.floor(payload.length / 3);
    socket.write(payload.slice(0, half), () => {
      setTimeout(() => {
        socket.write(payload.slice(half, half * 2), () => {
          setTimeout(() => socket.write(payload.slice(half * 2)), 10);
        });
      }, 10);
    });
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  t.after(() => transport[Symbol.asyncDispose]());

  const result = await transport.request<{ big: string }>("entry.list", {});
  assert.equal(result.big.length, 4096);
});

test("server disconnect rejects pending requests", async (t) => {
  const daemon = await startMockDaemon((_request, _socket) => {
    // Never respond; the test closes the server instead.
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  const pending = transport.request("session.list", {});

  // Wait for the request to reach the daemon, then kill the server.
  await new Promise((resolve) => setTimeout(resolve, 50));
  await daemon.close();

  await assert.rejects(pending, (error: unknown) => {
    assert.ok(error instanceof SessionError);
    assert.equal(error.code, "storage");
    return true;
  });

  await transport[Symbol.asyncDispose]();
});

test("dispose closes the socket and rejects further requests", async (t) => {
  const daemon = await startMockDaemon((request, socket) => {
    ok(socket, request.id, {});
  });
  t.after(() => daemon.close());

  const transport = new Transport({ socketPath: daemon.socketPath });
  await transport.request("session.list", {});
  assert.equal(transport.connected, true);

  await transport[Symbol.asyncDispose]();
  assert.equal(transport.connected, false);

  await assert.rejects(
    () => transport.request("session.list", {}),
    (error: unknown) => {
      assert.ok(error instanceof SessionError);
      assert.equal(error.code, "storage");
      return true;
    },
  );
});
