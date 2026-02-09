// Relay REST API client for the web app.

import type {
  CommentMessage,
  CommentThread,
  Document,
  Section,
  RelayErrorEnvelope as SharedRelayErrorEnvelope,
  Workspace,
} from "@scriptum/shared";
import { ScriptumApiClient, ScriptumApiError } from "@scriptum/shared";
import { getAccessToken as getAccessTokenFromAuth } from "./auth";
import { asRecord } from "./type-guards";

const DEFAULT_RELAY_URL =
  import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";

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
  addTags: (...args: SharedMethodArgs<"addTags">) => SharedMethodResult<"addTags">;
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
  updateShareLink: (
    ...args: SharedMethodArgs<"updateShareLink">
  ) => SharedMethodResult<"updateShareLink">;
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
        sharedClient.listWorkspaces({
          ...(listOptions.limit !== undefined
            ? { limit: listOptions.limit }
            : {}),
          ...(listOptions.cursor ? { cursor: listOptions.cursor } : {}),
        }, { signal: listOptions.signal }),
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
        sharedClient.listDocuments(workspaceId, {
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
        }, { signal: listOptions.signal }),
      );
      return mapPaged(payload, mapDocument);
    },

    createDocument: (...args) =>
      runWithApiError(() => sharedClient.createDocument(...args)),

    getDocument: async (workspaceId, documentId, getOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.getDocument(workspaceId, documentId, {
          ...(getOptions.includeContent !== undefined
            ? { include_content: getOptions.includeContent }
            : {}),
          ...(getOptions.includeSections !== undefined
            ? { include_sections: getOptions.includeSections }
            : {}),
        }, { signal: getOptions.signal }),
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

    createComment: async (workspaceId, documentId, input, requestOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.createComment(workspaceId, documentId, {
          anchor: {
            section_id: input.anchor.sectionId,
            start_offset_utf16: input.anchor.startOffsetUtf16,
            end_offset_utf16: input.anchor.endOffsetUtf16,
            head_seq: input.anchor.headSeq,
          },
          message: input.message,
        }, { signal: requestOptions.signal }),
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
        sharedClient.addCommentMessage(workspaceId, threadId, {
          body_md: bodyMd,
        }, { signal: requestOptions.signal }),
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
        sharedClient.resolveComment(workspaceId, threadId, {
          if_version: ifVersion,
        }, { signal: requestOptions.signal }),
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
        sharedClient.reopenComment(workspaceId, threadId, {
          if_version: ifVersion,
        }, { signal: requestOptions.signal }),
      );

      const record = asRecord(payload);
      return mapCommentThread(record?.thread);
    },
    createShareLink: (...args) =>
      runWithApiError(() => sharedClient.createShareLink(...args)),
    updateShareLink: (...args) =>
      runWithApiError(() => sharedClient.updateShareLink(...args)),
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

export function updateShareLink(
  ...args: SharedMethodArgs<"updateShareLink">
): SharedMethodResult<"updateShareLink"> {
  return getDefaultClient().updateShareLink(...args);
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
