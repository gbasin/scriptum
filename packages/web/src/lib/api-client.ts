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
}

export interface ListDocumentsOptions {
  limit?: number;
  cursor?: string;
  pathPrefix?: string;
  tag?: string;
  includeArchived?: boolean;
}

export interface GetDocumentOptions {
  includeContent?: boolean;
  includeSections?: boolean;
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

export interface ApiClientOptions {
  baseUrl?: string;
  fetch?: typeof fetch;
  getAccessToken?: () => Promise<string | null>;
  createIdempotencyKey?: () => string;
}

interface ApiClient {
  listWorkspaces: (
    options?: ListWorkspacesOptions,
  ) => Promise<PagedResult<Workspace>>;
  listDocuments: (
    workspaceId: string,
    options?: ListDocumentsOptions,
  ) => Promise<PagedResult<Document>>;
  getDocument: (
    workspaceId: string,
    documentId: string,
    options?: GetDocumentOptions,
  ) => Promise<GetDocumentResult>;
  updateDocument: (
    workspaceId: string,
    documentId: string,
    input: UpdateDocumentInput,
    options: { etag: string },
  ) => Promise<Document>;
  createComment: (
    workspaceId: string,
    documentId: string,
    input: CreateCommentInput,
  ) => Promise<{ thread: CommentThread; message: CommentMessage }>;
  addCommentMessage: (
    workspaceId: string,
    threadId: string,
    bodyMd: string,
  ) => Promise<CommentMessage>;
  resolveCommentThread: (
    workspaceId: string,
    threadId: string,
    ifVersion: number,
  ) => Promise<CommentThread>;
  reopenCommentThread: (
    workspaceId: string,
    threadId: string,
    ifVersion: number,
  ) => Promise<CommentThread>;
}

type UnknownRecord = Record<string, unknown>;

function asRecord(value: unknown): UnknownRecord | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  return value as UnknownRecord;
}

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
  });

  return {
    listWorkspaces: async (listOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.listWorkspaces({
          ...(listOptions.limit !== undefined
            ? { limit: listOptions.limit }
            : {}),
          ...(listOptions.cursor ? { cursor: listOptions.cursor } : {}),
        }),
      );
      return mapPaged(payload, mapWorkspace);
    },

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
        }),
      );
      return mapPaged(payload, mapDocument);
    },

    getDocument: async (workspaceId, documentId, getOptions = {}) => {
      const payload = await runWithApiError(() =>
        sharedClient.getDocument(workspaceId, documentId, {
          ...(getOptions.includeContent !== undefined
            ? { include_content: getOptions.includeContent }
            : {}),
          ...(getOptions.includeSections !== undefined
            ? { include_sections: getOptions.includeSections }
            : {}),
        }),
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
        }),
      );
      const record = asRecord(payload);
      return mapDocument(record?.document);
    },

    createComment: async (workspaceId, documentId, input) => {
      const payload = await runWithApiError(() =>
        sharedClient.createComment(workspaceId, documentId, {
          anchor: {
            section_id: input.anchor.sectionId,
            start_offset_utf16: input.anchor.startOffsetUtf16,
            end_offset_utf16: input.anchor.endOffsetUtf16,
            head_seq: input.anchor.headSeq,
          },
          message: input.message,
        }),
      );

      const record = asRecord(payload);
      return {
        thread: mapCommentThread(record?.thread),
        message: mapCommentMessage(record?.message),
      };
    },

    addCommentMessage: async (workspaceId, threadId, bodyMd) => {
      const payload = await runWithApiError(() =>
        sharedClient.addCommentMessage(workspaceId, threadId, {
          body_md: bodyMd,
        }),
      );

      const record = asRecord(payload);
      return mapCommentMessage(record?.message);
    },

    resolveCommentThread: async (workspaceId, threadId, ifVersion) => {
      const payload = await runWithApiError(() =>
        sharedClient.resolveComment(workspaceId, threadId, {
          if_version: ifVersion,
        }),
      );

      const record = asRecord(payload);
      return mapCommentThread(record?.thread);
    },

    reopenCommentThread: async (workspaceId, threadId, ifVersion) => {
      const payload = await runWithApiError(() =>
        sharedClient.reopenComment(workspaceId, threadId, {
          if_version: ifVersion,
        }),
      );

      const record = asRecord(payload);
      return mapCommentThread(record?.thread);
    },
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

export async function listWorkspaces(
  options?: ListWorkspacesOptions,
): Promise<PagedResult<Workspace>> {
  return getDefaultClient().listWorkspaces(options);
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
  options: { etag: string },
): Promise<Document> {
  return getDefaultClient().updateDocument(
    workspaceId,
    documentId,
    input,
    options,
  );
}

export async function createComment(
  workspaceId: string,
  documentId: string,
  input: CreateCommentInput,
): Promise<{ thread: CommentThread; message: CommentMessage }> {
  return getDefaultClient().createComment(workspaceId, documentId, input);
}

export async function addCommentMessage(
  workspaceId: string,
  threadId: string,
  bodyMd: string,
): Promise<CommentMessage> {
  return getDefaultClient().addCommentMessage(workspaceId, threadId, bodyMd);
}

export async function resolveCommentThread(
  workspaceId: string,
  threadId: string,
  ifVersion: number,
): Promise<CommentThread> {
  return getDefaultClient().resolveCommentThread(
    workspaceId,
    threadId,
    ifVersion,
  );
}

export async function reopenCommentThread(
  workspaceId: string,
  threadId: string,
  ifVersion: number,
): Promise<CommentThread> {
  return getDefaultClient().reopenCommentThread(
    workspaceId,
    threadId,
    ifVersion,
  );
}
