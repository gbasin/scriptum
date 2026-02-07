// Relay REST API client for the web app.

import type {
  CommentMessage,
  CommentThread,
  Document,
  Section,
  Workspace,
} from "@scriptum/shared";
import { getAccessToken as getAccessTokenFromAuth } from "./auth";

const DEFAULT_RELAY_URL =
  import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";

type HttpMethod = "GET" | "POST" | "PATCH" | "DELETE";
type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;

export interface RelayErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    retryable?: boolean;
    request_id?: string;
    requestId?: string;
    details?: unknown;
  };
}

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

interface RequestOptions {
  method?: HttpMethod;
  query?: QueryParams;
  body?: unknown;
  headers?: HeadersInit;
  ifMatch?: string;
  includeAuth?: boolean;
  idempotencyKey?: string;
}

interface ApiClient {
  listWorkspaces: (options?: ListWorkspacesOptions) => Promise<PagedResult<Workspace>>;
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
}

type UnknownRecord = Record<string, unknown>;

function asRecord(value: unknown): UnknownRecord | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  return value as UnknownRecord;
}

function readString(record: UnknownRecord | null, keys: readonly string[]): string | null {
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

function readNumber(record: UnknownRecord | null, keys: readonly string[]): number | null {
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

function readBoolean(record: UnknownRecord | null, keys: readonly string[]): boolean | null {
  if (!record) {
    return null;
  }
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "boolean") {
      return value;
    }
  }
  return null;
}

function readArray<T = unknown>(record: UnknownRecord | null, keys: readonly string[]): T[] {
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
    ...(readString(record, ["bodyMd", "body_md"]) ? { bodyMd: readString(record, ["bodyMd", "body_md"])! } : {}),
    tags,
    headSeq:
      readNumber(record, ["headSeq", "head_seq"]) ??
      (() => {
        throw new Error("Invalid document: missing headSeq/head_seq");
      })(),
    etag: requireString("document", record, ["etag"]),
    archivedAt:
      readNullableString(record, ["archivedAt", "archived_at"]) ?? null,
    deletedAt:
      readNullableString(record, ["deletedAt", "deleted_at"]) ?? null,
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
    createdBy: requireString("comment thread", record, [
      "createdBy",
      "created_by",
    ]),
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
  return {
    id: requireString("comment message", record, ["id"]),
    threadId: requireString("comment message", record, [
      "threadId",
      "thread_id",
    ]),
    author:
      requireString("comment message", record, ["author", "author_name"]),
    bodyMd: requireString("comment message", record, ["bodyMd", "body_md"]),
    createdAt: requireString("comment message", record, [
      "createdAt",
      "created_at",
    ]),
    editedAt: readNullableString(record, ["editedAt", "edited_at"]) ?? null,
  };
}

function mapPaged<T>(value: unknown, mapItem: (raw: unknown) => T): PagedResult<T> {
  const record = asRecord(value);
  const items = readArray(record, ["items"]).map((item) => mapItem(item));
  const nextCursor = readString(record, ["nextCursor", "next_cursor"]);
  return { items, nextCursor: nextCursor ?? null };
}

function normalizeBaseUrl(url: string): string {
  return url.replace(/\/+$/, "");
}

function createUrl(baseUrl: string, path: string, query?: QueryParams): URL {
  const url = new URL(path.startsWith("/") ? path : `/${path}`, `${baseUrl}/`);
  if (query) {
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === null) {
        continue;
      }
      url.searchParams.set(key, String(value));
    }
  }
  return url;
}

async function readJsonResponse<T>(response: Response): Promise<T | undefined> {
  if (response.status === 204) {
    return undefined;
  }
  const text = await response.text();
  if (text.length === 0) {
    return undefined;
  }
  return JSON.parse(text) as T;
}

async function parseApiError(
  response: Response,
  method: HttpMethod,
  url: string,
): Promise<ApiClientError> {
  const fallbackMessage = `Request failed (${response.status})`;
  let parsed: unknown;
  let message = fallbackMessage;
  let code: string | null = null;
  let retryable = response.status === 429 || response.status >= 500;
  let requestId: string | null = null;
  let details: unknown;

  try {
    parsed = await readJsonResponse<RelayErrorEnvelope>(response);
  } catch {
    parsed = undefined;
  }

  const envelope = asRecord(parsed);
  const errorRecord = asRecord(envelope?.error);

  message = readString(errorRecord, ["message"]) ?? fallbackMessage;
  code = readString(errorRecord, ["code"]);
  retryable = readBoolean(errorRecord, ["retryable"]) ?? retryable;
  requestId = readString(errorRecord, ["request_id", "requestId"]);
  details = errorRecord?.details;

  return new ApiClientError(
    response.status,
    method,
    url,
    code,
    message,
    retryable,
    requestId,
    details,
  );
}

export function createApiClient(options: ApiClientOptions = {}): ApiClient {
  const baseUrl = normalizeBaseUrl(options.baseUrl ?? DEFAULT_RELAY_URL);
  const fetchImpl = options.fetch ?? globalThis.fetch;
  const getAccessToken = options.getAccessToken ?? getAccessTokenFromAuth;
  const createIdempotencyKey =
    options.createIdempotencyKey ?? (() => crypto.randomUUID());

  const request = async <T>(path: string, requestOptions: RequestOptions = {}): Promise<T> => {
    const method = requestOptions.method ?? "GET";
    const url = createUrl(baseUrl, path, requestOptions.query);
    const headers = new Headers(requestOptions.headers);

    if (requestOptions.body !== undefined && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }
    if (requestOptions.ifMatch) {
      headers.set("If-Match", requestOptions.ifMatch);
    }

    if (requestOptions.includeAuth !== false && !headers.has("Authorization")) {
      const token = await getAccessToken();
      if (token) {
        headers.set("Authorization", `Bearer ${token}`);
      }
    }

    if (
      method === "POST" &&
      !url.pathname.startsWith("/v1/auth/") &&
      !headers.has("Idempotency-Key")
    ) {
      headers.set(
        "Idempotency-Key",
        requestOptions.idempotencyKey ?? createIdempotencyKey(),
      );
    }

    const response = await fetchImpl(url.toString(), {
      method,
      headers,
      body:
        requestOptions.body === undefined
          ? undefined
          : JSON.stringify(requestOptions.body),
    });

    if (!response.ok) {
      throw await parseApiError(response, method, url.toString());
    }

    return (await readJsonResponse<T>(response)) as T;
  };

  return {
    listWorkspaces: async (listOptions = {}) => {
      const payload = await request<unknown>("/v1/workspaces", {
        query: {
          ...(listOptions.limit !== undefined ? { limit: listOptions.limit } : {}),
          ...(listOptions.cursor ? { cursor: listOptions.cursor } : {}),
        },
      });
      return mapPaged(payload, mapWorkspace);
    },

    listDocuments: async (workspaceId, listOptions = {}) => {
      const payload = await request<unknown>(
        `/v1/workspaces/${encodeURIComponent(workspaceId)}/documents`,
        {
          query: {
            ...(listOptions.limit !== undefined ? { limit: listOptions.limit } : {}),
            ...(listOptions.cursor ? { cursor: listOptions.cursor } : {}),
            ...(listOptions.pathPrefix
              ? { path_prefix: listOptions.pathPrefix }
              : {}),
            ...(listOptions.tag ? { tag: listOptions.tag } : {}),
            ...(listOptions.includeArchived !== undefined
              ? { include_archived: listOptions.includeArchived }
              : {}),
          },
        },
      );
      return mapPaged(payload, mapDocument);
    },

    getDocument: async (workspaceId, documentId, getOptions = {}) => {
      const payload = await request<unknown>(
        `/v1/workspaces/${encodeURIComponent(
          workspaceId,
        )}/documents/${encodeURIComponent(documentId)}`,
        {
          query: {
            ...(getOptions.includeContent !== undefined
              ? { include_content: getOptions.includeContent }
              : {}),
            ...(getOptions.includeSections !== undefined
              ? { include_sections: getOptions.includeSections }
              : {}),
          },
        },
      );

      const record = asRecord(payload);
      const document = mapDocument(record?.document);
      const contentMd = readString(record, ["contentMd", "content_md"]);
      const sectionsRaw = readArray(record, ["sections"]);

      return {
        document,
        ...(contentMd ? { contentMd } : {}),
        ...(sectionsRaw.length > 0 ? { sections: sectionsRaw.map(mapSection) } : {}),
      };
    },

    updateDocument: async (workspaceId, documentId, input, updateOptions) => {
      const payload = await request<unknown>(
        `/v1/workspaces/${encodeURIComponent(
          workspaceId,
        )}/documents/${encodeURIComponent(documentId)}`,
        {
          method: "PATCH",
          ifMatch: updateOptions.etag,
          body: input,
        },
      );
      const record = asRecord(payload);
      return mapDocument(record?.document);
    },

    createComment: async (workspaceId, documentId, input) => {
      const payload = await request<unknown>(
        `/v1/workspaces/${encodeURIComponent(
          workspaceId,
        )}/documents/${encodeURIComponent(documentId)}/comments`,
        {
          method: "POST",
          body: {
            anchor: {
              section_id: input.anchor.sectionId,
              start_offset_utf16: input.anchor.startOffsetUtf16,
              end_offset_utf16: input.anchor.endOffsetUtf16,
              head_seq: input.anchor.headSeq,
            },
            message: input.message,
          },
        },
      );

      const record = asRecord(payload);
      return {
        thread: mapCommentThread(record?.thread),
        message: mapCommentMessage(record?.message),
      };
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
  return getDefaultClient().updateDocument(workspaceId, documentId, input, options);
}

export async function createComment(
  workspaceId: string,
  documentId: string,
  input: CreateCommentInput,
): Promise<{ thread: CommentThread; message: CommentMessage }> {
  return getDefaultClient().createComment(workspaceId, documentId, input);
}
