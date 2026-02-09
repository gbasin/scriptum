// Relay REST API client for the web app.

import type {
  CommentMessage,
  CommentThread,
  DocDiffResult,
  DocEditResult,
  DocHistoryResult,
  Document,
  GitConfigureResult,
  GitStatusResult,
  GitSyncPolicy,
  JsonRpcError,
  JsonRpcResponse,
  JsonRpcSuccessResponse,
  RpcMethod,
  RpcParamsMap,
  RpcResultMap,
  Section,
  RelayErrorEnvelope as SharedRelayErrorEnvelope,
  Workspace,
} from "@scriptum/shared";
import {
  CURRENT_RPC_PROTOCOL_VERSION,
  DAEMON_LOCAL_HOST,
  DAEMON_LOCAL_PORT,
  ScriptumApiClient,
  ScriptumApiError,
} from "@scriptum/shared";
import { getAccessToken as getAccessTokenFromAuth } from "./auth";
import { asRecord } from "./type-guards";

const DEFAULT_RELAY_URL =
  import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";
const DEFAULT_DAEMON_RPC_URL =
  import.meta.env.VITE_SCRIPTUM_DAEMON_RPC_URL ??
  `ws://${DAEMON_LOCAL_HOST}:${String(DAEMON_LOCAL_PORT)}/rpc`;
const DEFAULT_DAEMON_RPC_TIMEOUT_MS = 3_500;

type HttpMethod = "GET" | "POST" | "PATCH" | "DELETE";

export type RelayErrorEnvelope = SharedRelayErrorEnvelope;

export class ApiClientError extends Error {
  constructor(
    public readonly status: number,
    public readonly method: HttpMethod,
    public readonly url: string,
    public readonly code: string | null,
    message: string,
    public readonly retryable: boolean,
    public readonly requestId: string | null,
    public readonly details: unknown,
  ) {
    super(message);
    this.name = "ApiClientError";
  }
}

export interface DaemonRpcOptions {
  rpcUrl?: string;
  timeoutMs?: number;
}

function daemonClientUpdateId(prefix: string): string {
  if (
    typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
  ) {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export interface GitSyncSettingsSnapshot {
  branch: string;
  remoteUrl: string;
  pushPolicy: GitSyncPolicy;
  aiCommitEnabled: boolean;
  commitIntervalSeconds: number;
  dirty: boolean;
  ahead: number;
  behind: number;
  lastSyncAt: string | null;
}

export interface GitSyncSettingsUpdate {
  remoteUrl: string;
  branch: string;
  pushPolicy: GitSyncPolicy;
  aiCommitEnabled: boolean;
  commitIntervalSeconds: number;
}

export class DaemonRpcRequestError extends Error {
  constructor(
    message: string,
    public readonly code: number | null = null,
    public readonly data: unknown = undefined,
  ) {
    super(message);
    this.name = "DaemonRpcRequestError";
  }
}

function isGitSyncPolicy(value: unknown): value is GitSyncPolicy {
  return value === "disabled" || value === "manual" || value === "auto_rebase";
}

function asGitSyncPolicy(
  value: unknown,
  fallback: GitSyncPolicy = "manual",
): GitSyncPolicy {
  return isGitSyncPolicy(value) ? value : fallback;
}

function asPositiveInteger(value: unknown, fallback: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.max(1, Math.floor(value));
}

function asFiniteNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function asOptionalString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function isJsonRpcSuccessResponse<T>(
  value: JsonRpcResponse | null,
): value is JsonRpcSuccessResponse<RpcMethod> & { result: T } {
  return Boolean(value && "result" in value);
}

function pickResponseForRequestId(
  value: unknown,
  requestId: number,
): JsonRpcResponse | null {
  if (Array.isArray(value)) {
    for (const item of value) {
      const candidate = pickResponseForRequestId(item, requestId);
      if (candidate) {
        return candidate;
      }
    }
    return null;
  }

  const record = asRecord(value);
  if (!record) {
    return null;
  }

  const responseId = record.id;
  if (responseId !== requestId && responseId !== String(requestId)) {
    return null;
  }

  return record as unknown as JsonRpcResponse;
}

function daemonConnectionError(url: string): Error {
  return new Error(
    `Unable to connect to daemon RPC at ${url}. Confirm the daemon is running.`,
  );
}

export async function callDaemonRpc<M extends RpcMethod>(
  method: M,
  params: RpcParamsMap[M],
  options: DaemonRpcOptions = {},
): Promise<RpcResultMap[M]> {
  const rpcUrl = options.rpcUrl ?? DEFAULT_DAEMON_RPC_URL;
  const timeoutMs = options.timeoutMs ?? DEFAULT_DAEMON_RPC_TIMEOUT_MS;

  if (typeof WebSocket === "undefined") {
    throw daemonConnectionError(rpcUrl);
  }

  return new Promise<RpcResultMap[M]>((resolve, reject) => {
    const requestId = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);
    const requestPayload = JSON.stringify({
      jsonrpc: "2.0",
      protocol_version: CURRENT_RPC_PROTOCOL_VERSION,
      id: requestId,
      method,
      params,
    });

    const socket = new WebSocket(rpcUrl);
    let settled = false;
    let opened = false;

    const fail = (error: Error) => {
      if (settled) {
        return;
      }
      settled = true;
      globalThis.clearTimeout(timeoutHandle);
      try {
        socket.close();
      } catch {
        // no-op
      }
      reject(error);
    };

    const succeed = (result: RpcResultMap[M]) => {
      if (settled) {
        return;
      }
      settled = true;
      globalThis.clearTimeout(timeoutHandle);
      try {
        socket.close();
      } catch {
        // no-op
      }
      resolve(result);
    };

    const timeoutHandle = globalThis.setTimeout(() => {
      fail(
        new Error(
          `Timed out waiting for daemon RPC response (${timeoutMs}ms): ${method}`,
        ),
      );
    }, timeoutMs);

    socket.onopen = () => {
      opened = true;
      socket.send(requestPayload);
    };

    socket.onmessage = (event) => {
      const payload =
        typeof event.data === "string" ? event.data : String(event.data);
      let decoded: unknown;
      try {
        decoded = JSON.parse(payload);
      } catch {
        return;
      }

      const response = pickResponseForRequestId(decoded, requestId);
      if (!response) {
        return;
      }

      const errorPayload = (response as { error?: JsonRpcError }).error;
      if (errorPayload) {
        fail(
          new DaemonRpcRequestError(
            errorPayload.message,
            errorPayload.code,
            errorPayload.data,
          ),
        );
        return;
      }

      if (!isJsonRpcSuccessResponse<RpcResultMap[M]>(response)) {
        fail(new Error(`Daemon RPC response missing result for ${method}`));
        return;
      }

      succeed(response.result);
    };

    socket.onerror = () => {
      fail(daemonConnectionError(rpcUrl));
    };

    socket.onclose = () => {
      if (settled) {
        return;
      }
      fail(
        opened
          ? new Error("Daemon RPC connection closed before a response arrived.")
          : daemonConnectionError(rpcUrl),
      );
    };
  });
}

function normalizeGitStatusResult(
  status: GitStatusResult,
): GitSyncSettingsSnapshot {
  const commitInterval =
    status.commit_interval_seconds ?? status.commit_interval_sec ?? 30;

  return {
    branch: asOptionalString(status.branch) ?? "main",
    remoteUrl: asOptionalString(status.remote_url ?? status.remote) ?? "origin",
    pushPolicy: asGitSyncPolicy(status.push_policy ?? status.policy, "manual"),
    aiCommitEnabled:
      typeof status.ai_configured === "boolean"
        ? status.ai_configured
        : typeof status.ai_commit_enabled === "boolean"
          ? status.ai_commit_enabled
          : typeof status.ai_enabled === "boolean"
            ? status.ai_enabled
            : true,
    commitIntervalSeconds: asPositiveInteger(commitInterval, 30),
    dirty: Boolean(status.dirty),
    ahead: asFiniteNumber(status.ahead, 0),
    behind: asFiniteNumber(status.behind, 0),
    lastSyncAt: asOptionalString(status.last_sync_at),
  };
}

function normalizeGitConfigureResult(
  result: GitConfigureResult,
  fallback: GitSyncSettingsUpdate,
): GitSyncSettingsUpdate {
  const commitInterval =
    result.commit_interval_seconds ?? result.commit_interval_sec;

  return {
    remoteUrl: asOptionalString(result.remote_url) ?? fallback.remoteUrl,
    branch: asOptionalString(result.branch) ?? fallback.branch,
    pushPolicy: asGitSyncPolicy(
      result.push_policy ?? result.policy,
      fallback.pushPolicy,
    ),
    aiCommitEnabled:
      typeof result.ai_commit_enabled === "boolean"
        ? result.ai_commit_enabled
        : typeof result.ai_enabled === "boolean"
          ? result.ai_enabled
          : fallback.aiCommitEnabled,
    commitIntervalSeconds: asPositiveInteger(
      commitInterval,
      fallback.commitIntervalSeconds,
    ),
  };
}

export async function getGitSyncSettings(
  workspaceId: string,
  options?: DaemonRpcOptions,
): Promise<GitSyncSettingsSnapshot> {
  const result = await callDaemonRpc(
    "git.status",
    { workspace_id: workspaceId },
    options,
  );
  return normalizeGitStatusResult(result);
}

export async function configureGitSyncSettings(
  workspaceId: string,
  settings: GitSyncSettingsUpdate,
  options?: DaemonRpcOptions,
): Promise<GitSyncSettingsUpdate> {
  const sanitized: GitSyncSettingsUpdate = {
    remoteUrl: settings.remoteUrl.trim(),
    branch: settings.branch.trim() || "main",
    pushPolicy: settings.pushPolicy,
    aiCommitEnabled: settings.aiCommitEnabled,
    commitIntervalSeconds: asPositiveInteger(
      settings.commitIntervalSeconds,
      30,
    ),
  };

  const result = await callDaemonRpc(
    "git.configure",
    {
      workspace_id: workspaceId,
      remote_url: sanitized.remoteUrl,
      branch: sanitized.branch,
      push_policy: sanitized.pushPolicy,
      policy: sanitized.pushPolicy,
      ai_commit_enabled: sanitized.aiCommitEnabled,
      commit_interval_seconds: sanitized.commitIntervalSeconds,
    },
    options,
  );
  return normalizeGitConfigureResult(result, sanitized);
}

export async function getDocumentHistoryTimeline(
  workspaceId: string,
  documentId: string,
  options?: DaemonRpcOptions,
): Promise<DocHistoryResult> {
  return callDaemonRpc(
    "doc.history",
    {
      workspace_id: workspaceId,
      doc_id: documentId,
    },
    options,
  );
}

export interface DocumentDiffTimelineOptions extends DaemonRpcOptions {
  fromSeq?: number;
  toSeq?: number;
  granularity?: "snapshot" | "coarse" | "fine";
}

export async function getDocumentDiffTimeline(
  workspaceId: string,
  documentId: string,
  options: DocumentDiffTimelineOptions = {},
): Promise<DocDiffResult> {
  const { fromSeq, toSeq, granularity, rpcUrl, timeoutMs } = options;
  return callDaemonRpc(
    "doc.diff",
    {
      workspace_id: workspaceId,
      doc_id: documentId,
      ...(fromSeq === undefined ? {} : { from_seq: fromSeq }),
      ...(toSeq === undefined ? {} : { to_seq: toSeq }),
      ...(granularity === undefined ? {} : { granularity }),
    },
    {
      rpcUrl,
      timeoutMs,
    },
  );
}

export async function restoreDocumentFromSnapshot(
  workspaceId: string,
  documentId: string,
  contentMd: string,
  options?: DaemonRpcOptions,
): Promise<DocEditResult> {
  return callDaemonRpc(
    "doc.edit",
    {
      workspace_id: workspaceId,
      doc_id: documentId,
      client_update_id: daemonClientUpdateId("history-restore"),
      content_md: contentMd,
      agent_id: "web-history-restore",
    },
    options,
  );
}

export interface PagedResult<T> {
  items: T[];
  nextCursor: string | null;
}

export interface ListWorkspacesOptions {
  limit?: number;
  cursor?: string;
  signal?: AbortSignal;
}

export interface ListDocumentsOptions {
  limit?: number;
  cursor?: string;
  pathPrefix?: string;
  tag?: string;
  includeArchived?: boolean;
  signal?: AbortSignal;
}

export interface GetDocumentOptions {
  includeContent?: boolean;
  includeSections?: boolean;
  signal?: AbortSignal;
}

export interface GetDocumentResult {
  document: Document;
  contentMd?: string;
  sections?: Section[];
}

export interface CreateCommentInput {
  anchor: {
    sectionId: string | null;
    startOffsetUtf16: number;
    endOffsetUtf16: number;
    headSeq: number;
  };
  message: string;
}

export interface UpdateDocumentInput {
  title?: string;
  path?: string;
  archived?: boolean;
}

export interface ApiRequestOptions {
  signal?: AbortSignal;
}

export interface UpdateDocumentOptions extends ApiRequestOptions {
  etag: string;
}

export interface ApiClientOptions {
  baseUrl?: string;
  fetch?: typeof fetch;
  getAccessToken?: () => Promise<string | null>;
  createIdempotencyKey?: () => string;
  maxRetries?: number;
}

type SharedMethodArgs<K extends keyof ScriptumApiClient> = Parameters<
  ScriptumApiClient[K]
>;
type SharedMethodResult<K extends keyof ScriptumApiClient> = ReturnType<
  ScriptumApiClient[K]
>;

interface ApiClient {
  authOAuthStart: (
    ...args: SharedMethodArgs<"authOAuthStart">
  ) => SharedMethodResult<"authOAuthStart">;
  authOAuthCallback: (
    ...args: SharedMethodArgs<"authOAuthCallback">
  ) => SharedMethodResult<"authOAuthCallback">;
  authTokenRefresh: (
    ...args: SharedMethodArgs<"authTokenRefresh">
  ) => SharedMethodResult<"authTokenRefresh">;
  authLogout: (
    ...args: SharedMethodArgs<"authLogout">
  ) => SharedMethodResult<"authLogout">;
  listWorkspaces: (
    options?: ListWorkspacesOptions,
  ) => Promise<PagedResult<Workspace>>;
  createWorkspace: (
    ...args: SharedMethodArgs<"createWorkspace">
  ) => SharedMethodResult<"createWorkspace">;
  getWorkspace: (
    ...args: SharedMethodArgs<"getWorkspace">
  ) => SharedMethodResult<"getWorkspace">;
  updateWorkspace: (
    ...args: SharedMethodArgs<"updateWorkspace">
  ) => SharedMethodResult<"updateWorkspace">;
  inviteToWorkspace: (
    ...args: SharedMethodArgs<"inviteToWorkspace">
  ) => SharedMethodResult<"inviteToWorkspace">;
  acceptInvite: (
    ...args: SharedMethodArgs<"acceptInvite">
  ) => SharedMethodResult<"acceptInvite">;
  listMembers: (
    ...args: SharedMethodArgs<"listMembers">
  ) => SharedMethodResult<"listMembers">;
  updateMember: (
    ...args: SharedMethodArgs<"updateMember">
  ) => SharedMethodResult<"updateMember">;
  removeMember: (
    ...args: SharedMethodArgs<"removeMember">
  ) => SharedMethodResult<"removeMember">;
  listDocuments: (
    workspaceId: string,
    options?: ListDocumentsOptions,
  ) => Promise<PagedResult<Document>>;
  createDocument: (
    ...args: SharedMethodArgs<"createDocument">
  ) => SharedMethodResult<"createDocument">;
  getDocument: (
    workspaceId: string,
    documentId: string,
    options?: GetDocumentOptions,
  ) => Promise<GetDocumentResult>;
  updateDocument: (
    workspaceId: string,
    documentId: string,
    input: UpdateDocumentInput,
    options: UpdateDocumentOptions,
  ) => Promise<Document>;
  deleteDocument: (
    ...args: SharedMethodArgs<"deleteDocument">
  ) => SharedMethodResult<"deleteDocument">;
  addTags: (
    ...args: SharedMethodArgs<"addTags">
  ) => SharedMethodResult<"addTags">;
  searchDocuments: (
    ...args: SharedMethodArgs<"searchDocuments">
  ) => SharedMethodResult<"searchDocuments">;
  listComments: (
    ...args: SharedMethodArgs<"listComments">
  ) => SharedMethodResult<"listComments">;
  createComment: (
    workspaceId: string,
    documentId: string,
    input: CreateCommentInput,
    requestOptions?: ApiRequestOptions,
  ) => Promise<{ thread: CommentThread; message: CommentMessage }>;
  addCommentMessage: (
    workspaceId: string,
    threadId: string,
    bodyMd: string,
    requestOptions?: ApiRequestOptions,
  ) => Promise<CommentMessage>;
  resolveCommentThread: (
    workspaceId: string,
    threadId: string,
    ifVersion: number,
    requestOptions?: ApiRequestOptions,
  ) => Promise<CommentThread>;
  reopenCommentThread: (
    workspaceId: string,
    threadId: string,
    ifVersion: number,
    requestOptions?: ApiRequestOptions,
  ) => Promise<CommentThread>;
  createShareLink: (
    ...args: SharedMethodArgs<"createShareLink">
  ) => SharedMethodResult<"createShareLink">;
  listShareLinks: (
    ...args: SharedMethodArgs<"listShareLinks">
  ) => SharedMethodResult<"listShareLinks">;
  updateShareLink: (
    ...args: SharedMethodArgs<"updateShareLink">
  ) => SharedMethodResult<"updateShareLink">;
  revokeShareLink: (
    ...args: SharedMethodArgs<"revokeShareLink">
  ) => SharedMethodResult<"revokeShareLink">;
  redeemShareLink: (
    ...args: SharedMethodArgs<"redeemShareLink">
  ) => SharedMethodResult<"redeemShareLink">;
  createAclOverride: (
    ...args: SharedMethodArgs<"createAclOverride">
  ) => SharedMethodResult<"createAclOverride">;
  deleteAclOverride: (
    ...args: SharedMethodArgs<"deleteAclOverride">
  ) => SharedMethodResult<"deleteAclOverride">;
  createSyncSession: (
    ...args: SharedMethodArgs<"createSyncSession">
  ) => SharedMethodResult<"createSyncSession">;
}

type UnknownRecord = Record<string, unknown>;

function readString(
  record: UnknownRecord | null,
  keys: readonly string[],
): string | null {
  if (!record) {
    return null;
  }
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string") {
      return value;
    }
  }
  return null;
}

function readNullableString(
  record: UnknownRecord | null,
  keys: readonly string[],
): string | null | undefined {
  if (!record) {
    return undefined;
  }
  for (const key of keys) {
    if (!(key in record)) {
      continue;
    }
    const value = record[key];
    if (value === null) {
      return null;
    }
    if (typeof value === "string") {
      return value;
    }
  }
  return undefined;
}

function readNumber(
  record: UnknownRecord | null,
  keys: readonly string[],
): number | null {
  if (!record) {
    return null;
  }
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return null;
}

function readArray<T = unknown>(
  record: UnknownRecord | null,
  keys: readonly string[],
): T[] {
  if (!record) {
    return [];
  }
  for (const key of keys) {
    const value = record[key];
    if (Array.isArray(value)) {
      return value as T[];
    }
  }
  return [];
}

function requireString(
  entity: string,
  record: UnknownRecord | null,
  keys: readonly string[],
): string {
  const value = readString(record, keys);
  if (!value) {
    throw new Error(`Invalid ${entity}: missing ${keys.join("/")}`);
  }
  return value;
}

function mapWorkspace(value: unknown): Workspace {
  const record = asRecord(value);
  const configRecord = asRecord(record?.config);

  return {
    id: requireString("workspace", record, ["id"]),
    slug: requireString("workspace", record, ["slug"]),
    name: requireString("workspace", record, ["name"]),
    role: requireString("workspace", record, ["role"]),
    createdAt: requireString("workspace", record, ["createdAt", "created_at"]),
    updatedAt: requireString("workspace", record, ["updatedAt", "updated_at"]),
    etag: requireString("workspace", record, ["etag"]),
    ...(configRecord
      ? { config: configRecord as unknown as Workspace["config"] }
      : {}),
  };
}

function mapDocument(value: unknown): Document {
  const record = asRecord(value);
  const tags = readArray<string>(record, ["tags"]).filter(
    (tag): tag is string => typeof tag === "string",
  );

  return {
    id: requireString("document", record, ["id"]),
    workspaceId: requireString("document", record, [
      "workspaceId",
      "workspace_id",
    ]),
    path: requireString("document", record, ["path"]),
    title: requireString("document", record, ["title"]),
    ...(readString(record, ["bodyMd", "body_md"])
      ? { bodyMd: readString(record, ["bodyMd", "body_md"]) as string }
      : {}),
    tags,
    headSeq:
      readNumber(record, ["headSeq", "head_seq"]) ??
      (() => {
        throw new Error("Invalid document: missing headSeq/head_seq");
      })(),
    etag: requireString("document", record, ["etag"]),
    archivedAt:
      readNullableString(record, ["archivedAt", "archived_at"]) ?? null,
    deletedAt: readNullableString(record, ["deletedAt", "deleted_at"]) ?? null,
    createdAt: requireString("document", record, ["createdAt", "created_at"]),
    updatedAt: requireString("document", record, ["updatedAt", "updated_at"]),
  };
}

function mapSection(value: unknown): Section {
  const record = asRecord(value);
  return {
    id: requireString("section", record, ["id"]),
    parentId: readNullableString(record, ["parentId", "parent_id"]) ?? null,
    heading: requireString("section", record, ["heading"]),
    level:
      readNumber(record, ["level"]) ??
      (() => {
        throw new Error("Invalid section: missing level");
      })(),
    startLine:
      readNumber(record, ["startLine", "start_line"]) ??
      (() => {
        throw new Error("Invalid section: missing startLine/start_line");
      })(),
    endLine:
      readNumber(record, ["endLine", "end_line"]) ??
      (() => {
        throw new Error("Invalid section: missing endLine/end_line");
      })(),
  };
}

function mapCommentThread(value: unknown): CommentThread {
  const record = asRecord(value);
  const statusRaw = readString(record, ["status"]) ?? "open";
  const createdBy = readString(record, [
    "createdBy",
    "created_by",
    "created_by_user_id",
    "created_by_agent_id",
  ]);

  return {
    id: requireString("comment thread", record, ["id"]),
    docId: requireString("comment thread", record, ["docId", "doc_id"]),
    sectionId: readNullableString(record, ["sectionId", "section_id"]) ?? null,
    startOffsetUtf16:
      readNumber(record, ["startOffsetUtf16", "start_offset_utf16"]) ??
      (() => {
        throw new Error("Invalid comment thread: missing start offset");
      })(),
    endOffsetUtf16:
      readNumber(record, ["endOffsetUtf16", "end_offset_utf16"]) ??
      (() => {
        throw new Error("Invalid comment thread: missing end offset");
      })(),
    status: statusRaw === "resolved" ? "resolved" : "open",
    version:
      readNumber(record, ["version"]) ??
      (() => {
        throw new Error("Invalid comment thread: missing version");
      })(),
    createdBy:
      createdBy ??
      (() => {
        throw new Error("Invalid comment thread: missing createdBy/created_by");
      })(),
    createdAt: requireString("comment thread", record, [
      "createdAt",
      "created_at",
    ]),
    resolvedAt:
      readNullableString(record, ["resolvedAt", "resolved_at"]) ?? null,
  };
}

function mapCommentMessage(value: unknown): CommentMessage {
  const record = asRecord(value);
  const author = readString(record, [
    "author",
    "author_name",
    "author_user_id",
    "author_agent_id",
  ]);
  return {
    id: requireString("comment message", record, ["id"]),
    threadId: requireString("comment message", record, [
      "threadId",
      "thread_id",
    ]),
    author: author ?? "Unknown",
    bodyMd: requireString("comment message", record, ["bodyMd", "body_md"]),
    createdAt: requireString("comment message", record, [
      "createdAt",
      "created_at",
    ]),
    editedAt: readNullableString(record, ["editedAt", "edited_at"]) ?? null,
  };
}

function mapPaged<T>(
  value: unknown,
  mapItem: (raw: unknown) => T,
): PagedResult<T> {
  const record = asRecord(value);
  const items = readArray(record, ["items"]).map((item) => mapItem(item));
  const nextCursor = readString(record, ["nextCursor", "next_cursor"]);
  return { items, nextCursor: nextCursor ?? null };
}

function toApiClientError(error: ScriptumApiError): ApiClientError {
  return new ApiClientError(
    error.status,
    error.method,
    error.url,
    error.code,
    error.message,
    error.retryable,
    error.requestId,
    error.details,
  );
}

async function runWithApiError<T>(request: () => Promise<T>): Promise<T> {
  try {
    return await request();
  } catch (error) {
    if (error instanceof ScriptumApiError) {
      throw toApiClientError(error);
    }
    throw error;
  }
}

export function createApiClient(options: ApiClientOptions = {}): ApiClient {
  const sharedClient = new ScriptumApiClient({
    baseUrl: options.baseUrl ?? DEFAULT_RELAY_URL,
    ...(options.fetch ? { fetchImpl: options.fetch } : {}),
    tokenProvider: options.getAccessToken ?? getAccessTokenFromAuth,
    ...(options.createIdempotencyKey
      ? { idempotencyKeyFactory: options.createIdempotencyKey }
      : {}),
    ...(options.maxRetries !== undefined
      ? { maxRetries: options.maxRetries }
      : {}),
  });

  return {
    authOAuthStart: (...args) =>
      runWithApiError(() => sharedClient.authOAuthStart(...args)),
    authOAuthCallback: (...args) =>
      runWithApiError(() => sharedClient.authOAuthCallback(...args)),
    authTokenRefresh: (...args) =>
      runWithApiError(() => sharedClient.authTokenRefresh(...args)),
    authLogout: (...args) =>
      runWithApiError(() => sharedClient.authLogout(...args)),

    listWorkspaces: async (listOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.listWorkspaces(
          {
            ...(listOptions.limit !== undefined
              ? { limit: listOptions.limit }
              : {}),
            ...(listOptions.cursor ? { cursor: listOptions.cursor } : {}),
          },
          { signal: listOptions.signal },
        ),
      );
      return mapPaged(payload, mapWorkspace);
    },

    createWorkspace: (...args) =>
      runWithApiError(() => sharedClient.createWorkspace(...args)),
    getWorkspace: (...args) =>
      runWithApiError(() => sharedClient.getWorkspace(...args)),
    updateWorkspace: (...args) =>
      runWithApiError(() => sharedClient.updateWorkspace(...args)),
    inviteToWorkspace: (...args) =>
      runWithApiError(() => sharedClient.inviteToWorkspace(...args)),
    acceptInvite: (...args) =>
      runWithApiError(() => sharedClient.acceptInvite(...args)),
    listMembers: (...args) =>
      runWithApiError(() => sharedClient.listMembers(...args)),
    updateMember: (...args) =>
      runWithApiError(() => sharedClient.updateMember(...args)),
    removeMember: (...args) =>
      runWithApiError(() => sharedClient.removeMember(...args)),

    listDocuments: async (workspaceId, listOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.listDocuments(
          workspaceId,
          {
            ...(listOptions.limit !== undefined
              ? { limit: listOptions.limit }
              : {}),
            ...(listOptions.cursor ? { cursor: listOptions.cursor } : {}),
            ...(listOptions.pathPrefix
              ? { path_prefix: listOptions.pathPrefix }
              : {}),
            ...(listOptions.tag ? { tag: listOptions.tag } : {}),
            ...(listOptions.includeArchived !== undefined
              ? { include_archived: listOptions.includeArchived }
              : {}),
          },
          { signal: listOptions.signal },
        ),
      );
      return mapPaged(payload, mapDocument);
    },

    createDocument: (...args) =>
      runWithApiError(() => sharedClient.createDocument(...args)),

    getDocument: async (workspaceId, documentId, getOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.getDocument(
          workspaceId,
          documentId,
          {
            ...(getOptions.includeContent !== undefined
              ? { include_content: getOptions.includeContent }
              : {}),
            ...(getOptions.includeSections !== undefined
              ? { include_sections: getOptions.includeSections }
              : {}),
          },
          { signal: getOptions.signal },
        ),
      );

      const record = asRecord(payload);
      const document = mapDocument(record?.document);
      const contentMd = readString(record, ["contentMd", "content_md"]);
      const sectionsRaw = readArray(record, ["sections"]);

      return {
        document,
        ...(contentMd ? { contentMd } : {}),
        ...(sectionsRaw.length > 0
          ? { sections: sectionsRaw.map(mapSection) }
          : {}),
      };
    },

    updateDocument: async (workspaceId, documentId, input, updateOptions) => {
      const payload = await runWithApiError(() =>
        sharedClient.updateDocument(workspaceId, documentId, input, {
          ifMatch: updateOptions.etag,
          signal: updateOptions.signal,
        }),
      );
      const record = asRecord(payload);
      return mapDocument(record?.document);
    },

    deleteDocument: (...args) =>
      runWithApiError(() => sharedClient.deleteDocument(...args)),
    addTags: (...args) => runWithApiError(() => sharedClient.addTags(...args)),
    searchDocuments: (...args) =>
      runWithApiError(() => sharedClient.searchDocuments(...args)),
    listComments: (...args) =>
      runWithApiError(() => sharedClient.listComments(...args)),

    createComment: async (
      workspaceId,
      documentId,
      input,
      requestOptions = {},
    ) => {
      const payload = await runWithApiError(() =>
        sharedClient.createComment(
          workspaceId,
          documentId,
          {
            anchor: {
              section_id: input.anchor.sectionId,
              start_offset_utf16: input.anchor.startOffsetUtf16,
              end_offset_utf16: input.anchor.endOffsetUtf16,
              head_seq: input.anchor.headSeq,
            },
            message: input.message,
          },
          { signal: requestOptions.signal },
        ),
      );

      const record = asRecord(payload);
      return {
        thread: mapCommentThread(record?.thread),
        message: mapCommentMessage(record?.message),
      };
    },

    addCommentMessage: async (
      workspaceId,
      threadId,
      bodyMd,
      requestOptions = {},
    ) => {
      const payload = await runWithApiError(() =>
        sharedClient.addCommentMessage(
          workspaceId,
          threadId,
          {
            body_md: bodyMd,
          },
          { signal: requestOptions.signal },
        ),
      );

      const record = asRecord(payload);
      return mapCommentMessage(record?.message);
    },

    resolveCommentThread: async (
      workspaceId,
      threadId,
      ifVersion,
      requestOptions = {},
    ) => {
      const payload = await runWithApiError(() =>
        sharedClient.resolveComment(
          workspaceId,
          threadId,
          {
            if_version: ifVersion,
          },
          { signal: requestOptions.signal },
        ),
      );

      const record = asRecord(payload);
      return mapCommentThread(record?.thread);
    },

    reopenCommentThread: async (
      workspaceId,
      threadId,
      ifVersion,
      requestOptions = {},
    ) => {
      const payload = await runWithApiError(() =>
        sharedClient.reopenComment(
          workspaceId,
          threadId,
          {
            if_version: ifVersion,
          },
          { signal: requestOptions.signal },
        ),
      );

      const record = asRecord(payload);
      return mapCommentThread(record?.thread);
    },
    createShareLink: (...args) =>
      runWithApiError(() => sharedClient.createShareLink(...args)),
    listShareLinks: (...args) =>
      runWithApiError(() => sharedClient.listShareLinks(...args)),
    updateShareLink: (...args) =>
      runWithApiError(() => sharedClient.updateShareLink(...args)),
    revokeShareLink: (...args) =>
      runWithApiError(() => sharedClient.revokeShareLink(...args)),
    redeemShareLink: (...args) =>
      runWithApiError(() => sharedClient.redeemShareLink(...args)),
    createAclOverride: (...args) =>
      runWithApiError(() => sharedClient.createAclOverride(...args)),
    deleteAclOverride: (...args) =>
      runWithApiError(() => sharedClient.deleteAclOverride(...args)),
    createSyncSession: (...args) =>
      runWithApiError(() => sharedClient.createSyncSession(...args)),
  };
}

let defaultClient: ApiClient | null = null;

function getDefaultClient(): ApiClient {
  if (!defaultClient) {
    defaultClient = createApiClient();
  }
  return defaultClient;
}

export function resetApiClientForTests(): void {
  defaultClient = null;
}

export function authOAuthStart(
  ...args: SharedMethodArgs<"authOAuthStart">
): SharedMethodResult<"authOAuthStart"> {
  return getDefaultClient().authOAuthStart(...args);
}

export function authOAuthCallback(
  ...args: SharedMethodArgs<"authOAuthCallback">
): SharedMethodResult<"authOAuthCallback"> {
  return getDefaultClient().authOAuthCallback(...args);
}

export function authTokenRefresh(
  ...args: SharedMethodArgs<"authTokenRefresh">
): SharedMethodResult<"authTokenRefresh"> {
  return getDefaultClient().authTokenRefresh(...args);
}

export function authLogout(
  ...args: SharedMethodArgs<"authLogout">
): SharedMethodResult<"authLogout"> {
  return getDefaultClient().authLogout(...args);
}

export async function listWorkspaces(
  options?: ListWorkspacesOptions,
): Promise<PagedResult<Workspace>> {
  return getDefaultClient().listWorkspaces(options);
}

export function createWorkspace(
  ...args: SharedMethodArgs<"createWorkspace">
): SharedMethodResult<"createWorkspace"> {
  return getDefaultClient().createWorkspace(...args);
}

export function getWorkspace(
  ...args: SharedMethodArgs<"getWorkspace">
): SharedMethodResult<"getWorkspace"> {
  return getDefaultClient().getWorkspace(...args);
}

export function updateWorkspace(
  ...args: SharedMethodArgs<"updateWorkspace">
): SharedMethodResult<"updateWorkspace"> {
  return getDefaultClient().updateWorkspace(...args);
}

export function inviteToWorkspace(
  ...args: SharedMethodArgs<"inviteToWorkspace">
): SharedMethodResult<"inviteToWorkspace"> {
  return getDefaultClient().inviteToWorkspace(...args);
}

export function acceptInvite(
  ...args: SharedMethodArgs<"acceptInvite">
): SharedMethodResult<"acceptInvite"> {
  return getDefaultClient().acceptInvite(...args);
}

export function listMembers(
  ...args: SharedMethodArgs<"listMembers">
): SharedMethodResult<"listMembers"> {
  return getDefaultClient().listMembers(...args);
}

export function updateMember(
  ...args: SharedMethodArgs<"updateMember">
): SharedMethodResult<"updateMember"> {
  return getDefaultClient().updateMember(...args);
}

export function removeMember(
  ...args: SharedMethodArgs<"removeMember">
): SharedMethodResult<"removeMember"> {
  return getDefaultClient().removeMember(...args);
}

export async function listDocuments(
  workspaceId: string,
  options?: ListDocumentsOptions,
): Promise<PagedResult<Document>> {
  return getDefaultClient().listDocuments(workspaceId, options);
}

export async function getDocument(
  workspaceId: string,
  documentId: string,
  options?: GetDocumentOptions,
): Promise<GetDocumentResult> {
  return getDefaultClient().getDocument(workspaceId, documentId, options);
}

export async function updateDocument(
  workspaceId: string,
  documentId: string,
  input: UpdateDocumentInput,
  options: UpdateDocumentOptions,
): Promise<Document> {
  return getDefaultClient().updateDocument(
    workspaceId,
    documentId,
    input,
    options,
  );
}

export function createDocument(
  ...args: SharedMethodArgs<"createDocument">
): SharedMethodResult<"createDocument"> {
  return getDefaultClient().createDocument(...args);
}

export function deleteDocument(
  ...args: SharedMethodArgs<"deleteDocument">
): SharedMethodResult<"deleteDocument"> {
  return getDefaultClient().deleteDocument(...args);
}

export function addTags(
  ...args: SharedMethodArgs<"addTags">
): SharedMethodResult<"addTags"> {
  return getDefaultClient().addTags(...args);
}

export function searchDocuments(
  ...args: SharedMethodArgs<"searchDocuments">
): SharedMethodResult<"searchDocuments"> {
  return getDefaultClient().searchDocuments(...args);
}

export function listComments(
  ...args: SharedMethodArgs<"listComments">
): SharedMethodResult<"listComments"> {
  return getDefaultClient().listComments(...args);
}

export async function createComment(
  workspaceId: string,
  documentId: string,
  input: CreateCommentInput,
  requestOptions?: ApiRequestOptions,
): Promise<{ thread: CommentThread; message: CommentMessage }> {
  return getDefaultClient().createComment(
    workspaceId,
    documentId,
    input,
    requestOptions,
  );
}

export async function addCommentMessage(
  workspaceId: string,
  threadId: string,
  bodyMd: string,
  requestOptions?: ApiRequestOptions,
): Promise<CommentMessage> {
  return getDefaultClient().addCommentMessage(
    workspaceId,
    threadId,
    bodyMd,
    requestOptions,
  );
}

export async function resolveCommentThread(
  workspaceId: string,
  threadId: string,
  ifVersion: number,
  requestOptions?: ApiRequestOptions,
): Promise<CommentThread> {
  return getDefaultClient().resolveCommentThread(
    workspaceId,
    threadId,
    ifVersion,
    requestOptions,
  );
}

export async function reopenCommentThread(
  workspaceId: string,
  threadId: string,
  ifVersion: number,
  requestOptions?: ApiRequestOptions,
): Promise<CommentThread> {
  return getDefaultClient().reopenCommentThread(
    workspaceId,
    threadId,
    ifVersion,
    requestOptions,
  );
}

export function createShareLink(
  ...args: SharedMethodArgs<"createShareLink">
): SharedMethodResult<"createShareLink"> {
  return getDefaultClient().createShareLink(...args);
}

export function listShareLinks(
  ...args: SharedMethodArgs<"listShareLinks">
): SharedMethodResult<"listShareLinks"> {
  return getDefaultClient().listShareLinks(...args);
}

export function updateShareLink(
  ...args: SharedMethodArgs<"updateShareLink">
): SharedMethodResult<"updateShareLink"> {
  return getDefaultClient().updateShareLink(...args);
}

export function revokeShareLink(
  ...args: SharedMethodArgs<"revokeShareLink">
): SharedMethodResult<"revokeShareLink"> {
  return getDefaultClient().revokeShareLink(...args);
}

export function redeemShareLink(
  ...args: SharedMethodArgs<"redeemShareLink">
): SharedMethodResult<"redeemShareLink"> {
  return getDefaultClient().redeemShareLink(...args);
}

export function createAclOverride(
  ...args: SharedMethodArgs<"createAclOverride">
): SharedMethodResult<"createAclOverride"> {
  return getDefaultClient().createAclOverride(...args);
}

export function deleteAclOverride(
  ...args: SharedMethodArgs<"deleteAclOverride">
): SharedMethodResult<"deleteAclOverride"> {
  return getDefaultClient().deleteAclOverride(...args);
}

export function createSyncSession(
  ...args: SharedMethodArgs<"createSyncSession">
): SharedMethodResult<"createSyncSession"> {
  return getDefaultClient().createSyncSession(...args);
}
