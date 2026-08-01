import { connect, type Socket } from "node:net";
import { randomUUID } from "node:crypto";
import type { DaemonRequest, DaemonResponse } from "./types.js";
import { SessionError, toSessionErrorCode } from "./types.js";

export interface TransportOptions {
  socketPath: string;
  connectTimeoutMs?: number;
}

interface PendingRequest {
  resolve: (result: unknown) => void;
  reject: (error: SessionError) => void;
}

const DEFAULT_CONNECT_TIMEOUT_MS = 5_000;

export class Transport implements AsyncDisposable {
  private readonly socketPath: string;
  private readonly connectTimeoutMs: number;
  private socket: Socket | null = null;
  private buffer = "";
  private readonly pending = new Map<string, PendingRequest>();
  private connectPromise: Promise<void> | null = null;
  private disposed = false;

  constructor(options: TransportOptions) {
    this.socketPath = options.socketPath;
    this.connectTimeoutMs = options.connectTimeoutMs ?? DEFAULT_CONNECT_TIMEOUT_MS;
  }

  get connected(): boolean {
    return this.socket !== null && !this.socket.destroyed;
  }

  async request<T>(method: string, params: Record<string, unknown>): Promise<T> {
    if (this.disposed) {
      throw new SessionError("storage", "transport is disposed");
    }
    await this.ensureConnected();
    const id = randomUUID();
    const message: DaemonRequest & { id: string } = { id, method, params };
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, {
        resolve: resolve as (result: unknown) => void,
        reject,
      });
      const socket = this.socket;
      if (!socket) {
        this.pending.delete(id);
        reject(new SessionError("storage", "transport is not connected"));
        return;
      }
      socket.write(`${JSON.stringify(message)}\n`, (error) => {
        if (error) {
          this.pending.delete(id);
          reject(this.mapError(error));
        }
      });
    });
  }

  private ensureConnected(): Promise<void> {
    if (this.connected) {
      return Promise.resolve();
    }
    if (!this.connectPromise) {
      this.connectPromise = this.openSocket().finally(() => {
        this.connectPromise = null;
      });
    }
    return this.connectPromise;
  }

  private openSocket(): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = connect({ path: this.socketPath });
      let settled = false;

      const onConnectError = (error: NodeJS.ErrnoException) => {
        if (settled) return;
        settled = true;
        reject(this.mapError(error));
      };

      socket.once("error", onConnectError);
      socket.setTimeout(this.connectTimeoutMs, () => {
        socket.destroy(new Error(`connect timed out after ${this.connectTimeoutMs}ms`));
      });

      socket.once("connect", () => {
        if (settled) return;
        settled = true;
        socket.setTimeout(0);
        socket.removeListener("error", onConnectError);
        this.attach(socket);
        resolve();
      });
    });
  }

  private attach(socket: Socket): void {
    this.socket = socket;
    socket.setNoDelay(true);

    socket.on("data", (chunk: Buffer) => {
      this.buffer += chunk.toString("utf8");
      this.drainBuffer();
    });

    socket.on("error", () => {
      this.dropConnection();
    });

    socket.on("close", () => {
      this.dropConnection();
    });
  }

  private drainBuffer(): void {
    for (;;) {
      const newline = this.buffer.indexOf("\n");
      if (newline === -1) return;
      const line = this.buffer.slice(0, newline);
      this.buffer = this.buffer.slice(newline + 1);
      const trimmed = line.trim();
      if (trimmed.length === 0) continue;
      this.handleLine(trimmed);
    }
  }

  private handleLine(line: string): void {
    let response: DaemonResponse & { id?: string };
    try {
      response = JSON.parse(line) as DaemonResponse & { id?: string };
    } catch {
      return;
    }
    const id = response.id;
    if (typeof id !== "string") return;
    const pending = this.pending.get(id);
    if (!pending) return;
    this.pending.delete(id);
    if (response.ok) {
      pending.resolve(response.result);
    } else {
      const code = toSessionErrorCode(response.error?.code ?? "unknown");
      const message = response.error?.message ?? "daemon request failed";
      pending.reject(new SessionError(code, message));
    }
  }

  private dropConnection(): void {
    if (this.socket) {
      this.socket.removeAllListeners();
      this.socket.destroy();
      this.socket = null;
    }
    this.buffer = "";
    const error = new SessionError("storage", "connection to fabric daemon lost");
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  private mapError(error: unknown): SessionError {
    const errno = error as NodeJS.ErrnoException;
    if (errno && typeof errno === "object" && typeof errno.code === "string") {
      switch (errno.code) {
        case "ENOENT":
          return new SessionError(
            "storage",
            `fabric daemon socket not found at ${this.socketPath} (is the daemon running?)`,
            { cause: error },
          );
        case "ECONNREFUSED":
          return new SessionError(
            "storage",
            `connection refused by fabric daemon at ${this.socketPath}`,
            { cause: error },
          );
        default:
          return new SessionError("storage", `transport error: ${errno.message}`, {
            cause: error,
          });
      }
    }
    return new SessionError("storage", "transport error", { cause: error });
  }

  async [Symbol.asyncDispose](): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    await new Promise<void>((resolve) => {
      const socket = this.socket;
      if (!socket) {
        resolve();
        return;
      }
      socket.once("close", () => resolve());
      socket.end(() => {
        socket.destroy();
        resolve();
      });
    });
    this.dropConnection();
  }
}
