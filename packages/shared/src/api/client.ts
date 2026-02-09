import type {
  CommentMessage,
  CommentThread,
  Document,
  Section,
  ShareLink,
  SyncSession,
  Workspace,
} from "../index";
import {
  acceptInvite as acceptInvitePath,
  addCommentMessage as addCommentMessagePath,
  addTags as addTagsPath,
  authLogout as authLogoutPath,
  authOAuthCallback as authOAuthCallbackPath,
  authOAuthStart as authOAuthStartPath,
  authTokenRefresh as authTokenRefreshPath,
  createAclOverride as createAclOverridePath,
  createComment as createCommentPath,
  createDocument as createDocumentPath,
  createShareLink as createShareLinkPath,
  createSyncSession as createSyncSessionPath,
  createWorkspace as createWorkspacePath,
  deleteAclOverride as deleteAclOverridePath,
  deleteDocument as deleteDocumentPath,
  getDocument as getDocumentPath,
  getWorkspace as getWorkspacePath,
  inviteToWorkspace as inviteToWorkspacePath,
  listComments as listCommentsPath,
  listDocuments as listDocumentsPath,
  listMembers as listMembersPath,
  listWorkspaces as listWorkspacesPath,
  removeMember as removeMemberPath,
  reopenComment as reopenCommentPath,
  resolveComment as resolveCommentPath,
  searchDocuments as searchDocumentsPath,
  updateDocument as updateDocumentPath,
  updateMember as updateMemberPath,
  updateShareLink as updateShareLinkPath,
  updateWorkspace as updateWorkspacePath,
} from "./endpoints";

type HttpMethod = "GET" | "POST" | "PATCH" | "DELETE";
type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;

export interface ScriptumApiClientOptions {
  baseUrl: string;
  fetchImpl?: typeof fetch;
  tokenProvider?: (() => Promise<string | null>) | (() => string | null);
  idempotencyKeyFactory?: () => string;
  maxRetries?: number;
}

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

export class ScriptumApiError extends Error {
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
    this.name = "ScriptumApiError";
  }
}

export interface WorkspaceMember {
  user_id: string;
  email: string;
  role: string;
  status?: string;
  etag?: string;
}

export interface PagedResponse<T> {
  items: T[];
  next_cursor: string | null;
}

export interface OAuthStartRequest {
  redirect_uri: string;
  state: string;
  code_challenge: string;
  code_challenge_method: "S256";
}

export interface OAuthStartResponse {
  flow_id: string;
  authorization_url: string;
  expires_at: string;
}

export interface OAuthCallbackRequest {
  flow_id: string;
  code: string;
  state: string;
  code_verifier: string;
  device_name?: string;
}

export interface OAuthCallbackResponse {
  access_token: string;
  access_expires_at: string;
  refresh_token: string;
  refresh_expires_at: string;
  user: {
    id: string;
    email: string;
    display_name: string;
  };
}

export interface AuthRefreshRequest {
  refresh_token: string;
}

export type AuthRefreshResponse = OAuthCallbackResponse;

export interface AuthLogoutRequest {
  refresh_token: string;
}

export interface CreateWorkspaceRequest {
  name: string;
  slug: string;
}

export interface UpdateWorkspaceRequest {
  name?: string;
  slug?: string;
}

export interface InviteToWorkspaceRequest {
  email: string;
  role: string;
  expires_at?: string;
}

export interface AcceptInviteRequest {
  display_name: string;
}

export interface CreateDocumentRequest {
  path: string;
  title: string;
  content_md: string;
  tags?: string[];
}

export interface UpdateDocumentRequest {
  title?: string;
  path?: string;
  archived?: boolean;
}

export interface AddTagsRequest {
  op: "add" | "remove";
  tags: string[];
}

export interface SearchDocumentResult {
  doc_id: string;
  path: string;
  title: string;
  snippet: string;
  score: number;
}

export interface CreateCommentRequest {
  anchor: {
    section_id: string | null;
    start_offset_utf16: number;
    end_offset_utf16: number;
    head_seq: number;
  };
  message: string;
}

export interface AddCommentMessageRequest {
  body_md: string;
}

export interface ResolveCommentRequest {
  if_version: number;
}

export interface ReopenCommentRequest {
  if_version: number;
}

export interface CreateShareLinkRequest {
  target_type: "workspace" | "document";
  target_id: string;
  permission: "view" | "edit";
  expires_at?: string | null;
  max_uses?: number | null;
  password?: string;
}

export interface UpdateShareLinkRequest {
  expires_at?: string | null;
  max_uses?: number | null;
  disabled?: boolean;
}

export interface AclOverride {
  id: string;
  subject_type: "user" | "agent";
  subject_id: string;
  role: string;
  expires_at: string | null;
  created_at: string;
}

export interface CreateAclOverrideRequest {
  subject_type: "user" | "agent";
  subject_id: string;
  role: string;
  expires_at?: string | null;
}

export interface ApiRequestOptions {
  signal?: AbortSignal;
}

interface RequestOptions extends ApiRequestOptions {
  method?: HttpMethod;
  query?: QueryParams;
  body?: unknown;
  ifMatch?: string;
  includeAuth?: boolean;
}

const DEFAULT_MAX_RETRIES = 3;
const RETRY_BASE_DELAY_MS = 200;
const RETRY_MAX_DELAY_MS = 5_000;
const RETRY_JITTER_MS = 100;

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

function readString(
  record: Record<string, unknown> | null,
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

function readBoolean(
  record: Record<string, unknown> | null,
  keys: readonly string[],
): boolean | null {
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

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  return value as Record<string, unknown>;
}

async function parseJsonMaybe(response: Response): Promise<unknown> {
  if (response.status === 204) {
    return undefined;
  }
  const text = await response.text();
  if (text.length === 0) {
    return undefined;
  }
  return JSON.parse(text);
}

function normalizeMaxRetries(maxRetries: number | undefined): number {
  if (maxRetries === undefined) {
    return DEFAULT_MAX_RETRIES;
  }
  if (!Number.isFinite(maxRetries) || maxRetries < 0) {
    throw new RangeError("maxRetries must be a non-negative finite number");
  }
  return Math.trunc(maxRetries);
}

function parseRetryAfterMs(retryAfter: string | null): number | null {
  if (!retryAfter) {
    return null;
  }

  const asSeconds = Number(retryAfter);
  if (Number.isFinite(asSeconds) && asSeconds >= 0) {
    return Math.trunc(asSeconds * 1000);
  }

  const asDate = Date.parse(retryAfter);
  if (Number.isNaN(asDate)) {
    return null;
  }

  return Math.max(0, asDate - Date.now());
}

function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

export class ScriptumApiClient {
  private readonly baseUrl: string;
  private readonly fetchImpl: typeof fetch;
  private readonly tokenProvider:
    | (() => Promise<string | null>)
    | (() => string | null);
  private readonly idempotencyKeyFactory: () => string;
  private readonly maxRetries: number;

  constructor(options: ScriptumApiClientOptions) {
    this.baseUrl = normalizeBaseUrl(options.baseUrl);
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch;
    this.tokenProvider = options.tokenProvider ?? (() => null);
    this.idempotencyKeyFactory =
      options.idempotencyKeyFactory ?? (() => crypto.randomUUID());
    this.maxRetries = normalizeMaxRetries(options.maxRetries);
  }

  private buildUrl(path: string, query?: QueryParams): URL {
    const url = new URL(path, `${this.baseUrl}/`);
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

  private async parseError(
    response: Response,
    method: HttpMethod,
    url: string,
  ): Promise<ScriptumApiError> {
    const fallback = `Request failed (${response.status})`;
    let parsed: unknown;

    try {
      parsed = await parseJsonMaybe(response);
    } catch {
      parsed = undefined;
    }

    const envelope = asRecord(parsed);
    const errorRecord = asRecord(envelope?.error);

    const message = readString(errorRecord, ["message"]) ?? fallback;
    const code = readString(errorRecord, ["code"]);
    const retryable =
      readBoolean(errorRecord, ["retryable"]) ??
      (response.status === 429 || response.status >= 500);
    const requestId = readString(errorRecord, ["request_id", "requestId"]);
    const details = errorRecord?.details;

    return new ScriptumApiError(
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

  private async request<T>(
    path: string,
    options: RequestOptions = {},
  ): Promise<T> {
    const method = options.method ?? "GET";
    const url = this.buildUrl(path, options.query);
    const headers = new Headers();

    if (options.body !== undefined) {
      headers.set("Content-Type", "application/json");
    }

    if (options.ifMatch) {
      headers.set("If-Match", options.ifMatch);
    }

    if (options.includeAuth !== false) {
      const token = await this.tokenProvider();
      if (token) {
        headers.set("Authorization", `Bearer ${token}`);
      }
    }

    if (method === "POST" && !url.pathname.startsWith("/v1/auth/")) {
      headers.set("Idempotency-Key", this.idempotencyKeyFactory());
    }

    for (let attempt = 0; ; attempt += 1) {
      try {
        const response = await this.fetchImpl(url.toString(), {
          method,
          headers,
          body:
            options.body === undefined
              ? undefined
              : JSON.stringify(options.body),
          signal: options.signal,
        });

        if (response.ok) {
          return (await parseJsonMaybe(response)) as T;
        }

        const retryAfterMs = parseRetryAfterMs(
          response.headers.get("Retry-After"),
        );
        const apiError = await this.parseError(
          response,
          method,
          url.toString(),
        );
        if (
          attempt < this.maxRetries &&
          apiError.retryable &&
          (response.status === 429 || response.status >= 500)
        ) {
          await this.sleep(
            this.computeRetryDelayMs(attempt, retryAfterMs),
            options.signal,
          );
          continue;
        }
        throw apiError;
      } catch (error) {
        if (error instanceof ScriptumApiError) {
          throw error;
        }
        if (
          attempt < this.maxRetries &&
          error instanceof TypeError &&
          !isAbortError(error)
        ) {
          await this.sleep(this.computeRetryDelayMs(attempt), options.signal);
          continue;
        }
        throw error;
      }
    }
  }

  private computeRetryDelayMs(
    attempt: number,
    retryAfterMs: number | null = null,
  ): number {
    const exponentialDelay = Math.min(
      RETRY_BASE_DELAY_MS * 2 ** attempt,
      RETRY_MAX_DELAY_MS,
    );
    const jitter = Math.floor(Math.random() * (RETRY_JITTER_MS + 1));
    const computedDelay = exponentialDelay + jitter;
    return retryAfterMs === null
      ? computedDelay
      : Math.max(computedDelay, retryAfterMs);
  }

  private createAbortError(): Error {
    if (typeof DOMException !== "undefined") {
      return new DOMException("The operation was aborted", "AbortError");
    }
    const error = new Error("The operation was aborted");
    error.name = "AbortError";
    return error;
  }

  private async sleep(delayMs: number, signal?: AbortSignal): Promise<void> {
    if (delayMs <= 0) {
      return;
    }
    if (signal?.aborted) {
      throw this.createAbortError();
    }
    await new Promise<void>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        signal?.removeEventListener("abort", onAbort);
        resolve();
      }, delayMs);

      const onAbort = () => {
        clearTimeout(timeoutId);
        signal?.removeEventListener("abort", onAbort);
        reject(this.createAbortError());
      };

      signal?.addEventListener("abort", onAbort, { once: true });
    });
  }

  authOAuthStart(
    body: OAuthStartRequest,
    options: ApiRequestOptions = {},
  ): Promise<OAuthStartResponse> {
    return this.request(authOAuthStartPath(), {
      method: "POST",
      body,
      includeAuth: false,
      signal: options.signal,
    });
  }

  authOAuthCallback(
    body: OAuthCallbackRequest,
    options: ApiRequestOptions = {},
  ): Promise<OAuthCallbackResponse> {
    return this.request(authOAuthCallbackPath(), {
      method: "POST",
      body,
      includeAuth: false,
      signal: options.signal,
    });
  }

  authTokenRefresh(
    body: AuthRefreshRequest,
    options: ApiRequestOptions = {},
  ): Promise<AuthRefreshResponse> {
    return this.request(authTokenRefreshPath(), {
      method: "POST",
      body,
      includeAuth: false,
      signal: options.signal,
    });
  }

  async authLogout(
    body: AuthLogoutRequest,
    options: ApiRequestOptions = {},
  ): Promise<void> {
    await this.request<void>(authLogoutPath(), {
      method: "POST",
      body,
      includeAuth: true,
      signal: options.signal,
    });
  }

  listWorkspaces(
    params?: {
      limit?: number;
      cursor?: string;
    },
    options: ApiRequestOptions = {},
  ): Promise<PagedResponse<Workspace>> {
    return this.request(listWorkspacesPath(), {
      query: params,
      signal: options.signal,
    });
  }

  createWorkspace(
    body: CreateWorkspaceRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ workspace: Workspace }> {
    return this.request(createWorkspacePath(), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  getWorkspace(
    workspaceId: string,
    options: ApiRequestOptions = {},
  ): Promise<{ workspace: Workspace }> {
    return this.request(getWorkspacePath(workspaceId), {
      signal: options.signal,
    });
  }

  updateWorkspace(
    workspaceId: string,
    body: UpdateWorkspaceRequest,
    options: { ifMatch?: string; signal?: AbortSignal } = {},
  ): Promise<{ workspace: Workspace }> {
    return this.request(updateWorkspacePath(workspaceId), {
      method: "PATCH",
      body,
      ifMatch: options.ifMatch,
      signal: options.signal,
    });
  }

  inviteToWorkspace(
    workspaceId: string,
    body: InviteToWorkspaceRequest,
    options: ApiRequestOptions = {},
  ): Promise<{
    invite_id: string;
    email: string;
    role: string;
    expires_at: string | null;
    status: string;
  }> {
    return this.request(inviteToWorkspacePath(workspaceId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  acceptInvite(
    token: string,
    body: AcceptInviteRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ workspace: Workspace; member: WorkspaceMember }> {
    return this.request(acceptInvitePath(token), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  listMembers(
    workspaceId: string,
    params?: { limit?: number; cursor?: string },
    options: ApiRequestOptions = {},
  ): Promise<PagedResponse<WorkspaceMember>> {
    return this.request(listMembersPath(workspaceId), {
      query: params,
      signal: options.signal,
    });
  }

  updateMember(
    workspaceId: string,
    userId: string,
    body: { role?: string; status?: string },
    options: { ifMatch?: string; signal?: AbortSignal } = {},
  ): Promise<WorkspaceMember> {
    return this.request(updateMemberPath(workspaceId, userId), {
      method: "PATCH",
      body,
      ifMatch: options.ifMatch,
      signal: options.signal,
    });
  }

  async removeMember(
    workspaceId: string,
    userId: string,
    options: { ifMatch?: string; signal?: AbortSignal } = {},
  ): Promise<void> {
    await this.request<void>(removeMemberPath(workspaceId, userId), {
      method: "DELETE",
      ifMatch: options.ifMatch,
      signal: options.signal,
    });
  }

  listDocuments(
    workspaceId: string,
    params?: {
      limit?: number;
      cursor?: string;
      path_prefix?: string;
      tag?: string;
      include_archived?: boolean;
    },
    options: ApiRequestOptions = {},
  ): Promise<PagedResponse<Document>> {
    return this.request(listDocumentsPath(workspaceId), {
      query: params,
      signal: options.signal,
    });
  }

  createDocument(
    workspaceId: string,
    body: CreateDocumentRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ document: Document; sections: Section[]; etag: string }> {
    return this.request(createDocumentPath(workspaceId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  getDocument(
    workspaceId: string,
    documentId: string,
    params?: { include_content?: boolean; include_sections?: boolean },
    options: ApiRequestOptions = {},
  ): Promise<{
    document: Document;
    content_md?: string;
    sections?: Section[];
  }> {
    return this.request(getDocumentPath(workspaceId, documentId), {
      query: params,
      signal: options.signal,
    });
  }

  updateDocument(
    workspaceId: string,
    documentId: string,
    body: UpdateDocumentRequest,
    options: { ifMatch?: string; signal?: AbortSignal } = {},
  ): Promise<{ document: Document }> {
    return this.request(updateDocumentPath(workspaceId, documentId), {
      method: "PATCH",
      body,
      ifMatch: options.ifMatch,
      signal: options.signal,
    });
  }

  async deleteDocument(
    workspaceId: string,
    documentId: string,
    params?: { hard_delete?: boolean },
    options: { ifMatch?: string; signal?: AbortSignal } = {},
  ): Promise<void> {
    await this.request<void>(deleteDocumentPath(workspaceId, documentId), {
      method: "DELETE",
      query: params,
      ifMatch: options.ifMatch,
      signal: options.signal,
    });
  }

  addTags(
    workspaceId: string,
    documentId: string,
    body: AddTagsRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ document: Document }> {
    return this.request(addTagsPath(workspaceId, documentId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  searchDocuments(
    workspaceId: string,
    params: { q: string; limit?: number; cursor?: string },
    options: ApiRequestOptions = {},
  ): Promise<PagedResponse<SearchDocumentResult>> {
    return this.request(searchDocumentsPath(workspaceId), {
      query: params,
      signal: options.signal,
    });
  }

  listComments(
    workspaceId: string,
    documentId: string,
    params?: { status?: "open" | "resolved"; limit?: number; cursor?: string },
    options: ApiRequestOptions = {},
  ): Promise<
    PagedResponse<{ thread: CommentThread; messages: CommentMessage[] }>
  > {
    return this.request(listCommentsPath(workspaceId, documentId), {
      query: params,
      signal: options.signal,
    });
  }

  createComment(
    workspaceId: string,
    documentId: string,
    body: CreateCommentRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ thread: CommentThread; message: CommentMessage }> {
    return this.request(createCommentPath(workspaceId, documentId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  addCommentMessage(
    workspaceId: string,
    commentId: string,
    body: AddCommentMessageRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ message: CommentMessage }> {
    return this.request(addCommentMessagePath(workspaceId, commentId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  resolveComment(
    workspaceId: string,
    commentId: string,
    body: ResolveCommentRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ thread: CommentThread }> {
    return this.request(resolveCommentPath(workspaceId, commentId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  reopenComment(
    workspaceId: string,
    commentId: string,
    body: ReopenCommentRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ thread: CommentThread }> {
    return this.request(reopenCommentPath(workspaceId, commentId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  createShareLink(
    workspaceId: string,
    body: CreateShareLinkRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ share_link: ShareLink }> {
    return this.request(createShareLinkPath(workspaceId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  updateShareLink(
    workspaceId: string,
    shareLinkId: string,
    body: UpdateShareLinkRequest,
    options: { ifMatch?: string; signal?: AbortSignal } = {},
  ): Promise<{ share_link: ShareLink }> {
    return this.request(updateShareLinkPath(workspaceId, shareLinkId), {
      method: "PATCH",
      body,
      ifMatch: options.ifMatch,
      signal: options.signal,
    });
  }

  createAclOverride(
    workspaceId: string,
    documentId: string,
    body: CreateAclOverrideRequest,
    options: ApiRequestOptions = {},
  ): Promise<{ acl_override: AclOverride }> {
    return this.request(createAclOverridePath(workspaceId, documentId), {
      method: "POST",
      body,
      signal: options.signal,
    });
  }

  async deleteAclOverride(
    workspaceId: string,
    documentId: string,
    overrideId: string,
    options: ApiRequestOptions = {},
  ): Promise<void> {
    await this.request<void>(
      deleteAclOverridePath(workspaceId, documentId, overrideId),
      {
        method: "DELETE",
        signal: options.signal,
      },
    );
  }

  createSyncSession(
    workspaceId: string,
    options: ApiRequestOptions = {},
  ): Promise<SyncSession> {
    return this.request(createSyncSessionPath(workspaceId), {
      method: "POST",
      signal: options.signal,
    });
  }
}
