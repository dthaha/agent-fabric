import { FabricSessionReader } from "./fabric-reader.js";
import { Transport } from "./transport.js";
import {
  DEFAULT_SOCKET_PATH,
  type SessionCreateOptions,
  type SessionForkSelection,
  type SessionMetadata,
  type SessionReader,
  type SessionStore,
  type SessionTreeEntry,
} from "./types.js";

export interface FabricSessionStoreOptions {
  socketPath?: string;
}

interface DaemonSessionMetadata {
  id: string;
  created_at: string;
}

function toMetadata(raw: DaemonSessionMetadata): SessionMetadata {
  return { id: raw.id, createdAt: raw.created_at };
}

export class FabricSessionStore implements SessionStore {
  private readonly transport: Transport;

  constructor(options: FabricSessionStoreOptions = {}) {
    const socketPath =
      options.socketPath ?? process.env.FABRIC_SOCKET_PATH ?? DEFAULT_SOCKET_PATH;
    this.transport = new Transport({ socketPath });
  }

  async create(options: SessionCreateOptions): Promise<SessionReader> {
    const params: Record<string, unknown> = {};
    if (options.id !== undefined) {
      params.id = options.id;
    }
    const metadata = toMetadata(
      await this.transport.request<DaemonSessionMetadata>("session.create", params),
    );
    return new FabricSessionReader(metadata, this.transport);
  }

  async load(metadata: SessionMetadata): Promise<SessionReader> {
    const loaded = toMetadata(
      await this.transport.request<DaemonSessionMetadata>("session.load", {
        id: metadata.id,
      }),
    );
    return new FabricSessionReader(loaded, this.transport);
  }

  async list(): Promise<SessionMetadata[]> {
    const result = await this.transport.request<{ sessions: DaemonSessionMetadata[] }>(
      "session.list",
      {},
    );
    return result.sessions.map(toMetadata);
  }

  async appendEntry(metadata: SessionMetadata, entry: SessionTreeEntry): Promise<void> {
    await this.transport.request("entry.append", {
      session_id: metadata.id,
      entry,
    });
  }

  async delete(metadata: SessionMetadata): Promise<void> {
    await this.transport.request("session.delete", { id: metadata.id });
  }

  async fork(
    source: SessionMetadata,
    options: SessionCreateOptions,
    selection: SessionForkSelection,
  ): Promise<SessionReader> {
    const params: Record<string, unknown> = {
      source_id: source.id,
      selection,
    };
    if (options.id !== undefined) {
      params.id = options.id;
    }
    const metadata = toMetadata(
      await this.transport.request<DaemonSessionMetadata>("session.fork", params),
    );
    return new FabricSessionReader(metadata, this.transport);
  }

  async [Symbol.asyncDispose](): Promise<void> {
    await this.transport[Symbol.asyncDispose]();
  }
}
