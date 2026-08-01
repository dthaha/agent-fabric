export type SessionErrorCode =
  | "not_found"
  | "invalid_session"
  | "invalid_entry"
  | "invalid_fork_target"
  | "storage"
  | "unknown";

export class SessionError extends Error {
  readonly code: SessionErrorCode;

  constructor(code: SessionErrorCode, message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "SessionError";
    this.code = code;
  }
}

export interface SessionMetadata {
  id: string;
  createdAt: string;
}

export interface SessionCreateOptions {
  id?: string;
}

export interface SessionEntryCursorOptions {
  afterEntrySeq?: number;
  limit?: number;
}

export type SessionForkSelection =
  | { kind: "all" }
  | { kind: "before_user_message"; entryId: string }
  | { kind: "through_entry"; entryId: string };

export interface SessionTreeEntryBase {
  type: string;
  id: string;
  parentId: string | null;
  timestamp: string;
  [key: string]: unknown;
}

export type SessionTreeEntry = SessionTreeEntryBase;

export interface SessionHead {
  leafId: string | null;
}

export interface SessionReader {
  readonly metadata: SessionMetadata;
  readHead(): Promise<SessionHead>;
  readEntry(id: string): Promise<SessionTreeEntry | undefined>;
  readEntries(options?: SessionEntryCursorOptions): Promise<SessionTreeEntry[]>;
  readPathToRootOrCompaction(leafId: string | null): Promise<SessionTreeEntry[]>;
}

export interface SessionStore extends AsyncDisposable {
  create(options: SessionCreateOptions): Promise<SessionReader>;
  load(metadata: SessionMetadata): Promise<SessionReader>;
  list(options?: void): Promise<SessionMetadata[]>;
  appendEntry(metadata: SessionMetadata, entry: SessionTreeEntry): Promise<void>;
  delete(metadata: SessionMetadata): Promise<void>;
  fork(
    source: SessionMetadata,
    options: SessionCreateOptions,
    selection: SessionForkSelection,
  ): Promise<SessionReader>;
}

export interface DaemonRequest {
  method: string;
  params: Record<string, unknown>;
}

export interface DaemonResponse {
  ok: boolean;
  result?: unknown;
  error?: { code: string; message: string };
}

export const DEFAULT_SOCKET_PATH = "/tmp/fabric-endpoint.sock";

const ERROR_CODES: ReadonlySet<string> = new Set([
  "not_found",
  "invalid_session",
  "invalid_entry",
  "invalid_fork_target",
  "storage",
  "unknown",
]);

export function toSessionErrorCode(code: string): SessionErrorCode {
  return ERROR_CODES.has(code) ? (code as SessionErrorCode) : "unknown";
}
