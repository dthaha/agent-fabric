import type { Transport } from "./transport.js";
import type {
  SessionEntryCursorOptions,
  SessionHead,
  SessionMetadata,
  SessionReader,
  SessionTreeEntry,
} from "./types.js";

export class FabricSessionReader implements SessionReader {
  readonly metadata: SessionMetadata;

  private readonly transport: Transport;

  constructor(metadata: SessionMetadata, transport: Transport) {
    this.metadata = metadata;
    this.transport = transport;
  }

  async readHead(): Promise<SessionHead> {
    const result = await this.transport.request<{ leaf_id: string | null }>(
      "session.head",
      { session_id: this.metadata.id },
    );
    return { leafId: result.leaf_id };
  }

  async readEntry(id: string): Promise<SessionTreeEntry | undefined> {
    const result = await this.transport.request<{ entry: SessionTreeEntry | null }>(
      "entry.read",
      { session_id: this.metadata.id, id },
    );
    return result.entry ?? undefined;
  }

  async readEntries(options?: SessionEntryCursorOptions): Promise<SessionTreeEntry[]> {
    const params: Record<string, unknown> = { session_id: this.metadata.id };
    if (options?.afterEntrySeq !== undefined) {
      params.after_seq = options.afterEntrySeq;
    }
    if (options?.limit !== undefined) {
      params.limit = options.limit;
    }
    const result = await this.transport.request<{ entries: SessionTreeEntry[] }>(
      "entry.list",
      params,
    );
    return result.entries;
  }

  async readPathToRootOrCompaction(leafId: string | null): Promise<SessionTreeEntry[]> {
    const result = await this.transport.request<{ entries: SessionTreeEntry[] }>(
      "entry.path",
      { session_id: this.metadata.id, leaf_id: leafId },
    );
    return result.entries;
  }
}
