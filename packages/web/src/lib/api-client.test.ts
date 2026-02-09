import { describe, expect, it, vi } from "vitest";
import {
  type ApiClientError,
  type CreateCommentInput,
  createApiClient,
} from "./api-client";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function headerValue(
  init: RequestInit | undefined,
  key: string,
): string | null {
  return new Headers(init?.headers).get(key);
}

describe("api-client", () => {
  it("injects Authorization header for authenticated requests", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(200, {
        items: [
          {
            id: "w1",
            slug: "alpha",
            name: "Alpha",
            role: "editor",
            created_at: "2026-02-07T00:00:00.000Z",
            updated_at: "2026-02-07T00:01:00.000Z",
            etag: "etag-w1",
          },
        ],
        next_cursor: "next-1",
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-abc",
    });
    const controller = new AbortController();

    const response = await client.listWorkspaces({
      limit: 10,
      cursor: "cur-1",
      signal: controller.signal,
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;

    expect(url).toBe(
      "https://relay.scriptum.dev/v1/workspaces?limit=10&cursor=cur-1",
    );
    expect(init?.method).toBe("GET");
    expect(init?.signal).toBe(controller.signal);
    expect(headerValue(init, "Authorization")).toBe("Bearer token-abc");
    expect(response).toEqual({
      items: [
        {
          id: "w1",
          slug: "alpha",
          name: "Alpha",
          role: "editor",
          createdAt: "2026-02-07T00:00:00.000Z",
          updatedAt: "2026-02-07T00:01:00.000Z",
          etag: "etag-w1",
        },
      ],
      nextCursor: "next-1",
    });
  });

  it("adds Idempotency-Key and serializes JSON for mutating POST", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(201, {
        thread: {
          id: "thread-1",
          doc_id: "doc-1",
          section_id: "s-1",
          start_offset_utf16: 12,
          end_offset_utf16: 24,
          status: "open",
          version: 1,
          created_by: "user-1",
          created_at: "2026-02-07T00:00:00.000Z",
          resolved_at: null,
        },
        message: {
          id: "msg-1",
          thread_id: "thread-1",
          author_name: "Alice",
          body_md: "Looks good",
          created_at: "2026-02-07T00:00:00.000Z",
          edited_at: null,
        },
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-xyz",
      createIdempotencyKey: () => "idem-123",
    });

    const input: CreateCommentInput = {
      anchor: {
        sectionId: "s-1",
        startOffsetUtf16: 12,
        endOffsetUtf16: 24,
        headSeq: 7,
      },
      message: "Looks good",
    };
    const controller = new AbortController();

    const response = await client.createComment("workspace-1", "doc-1", input, {
      signal: controller.signal,
    });

    const [, init] = fetchMock.mock.calls[0]!;
    expect(init?.method).toBe("POST");
    expect(init?.signal).toBe(controller.signal);
    expect(headerValue(init, "Authorization")).toBe("Bearer token-xyz");
    expect(headerValue(init, "Idempotency-Key")).toBe("idem-123");
    expect(headerValue(init, "Content-Type")).toBe("application/json");
    expect(init?.body).toBe(
      JSON.stringify({
        anchor: {
          section_id: "s-1",
          start_offset_utf16: 12,
          end_offset_utf16: 24,
          head_seq: 7,
        },
        message: "Looks good",
      }),
    );
    expect(response).toEqual({
      thread: {
        id: "thread-1",
        docId: "doc-1",
        sectionId: "s-1",
        startOffsetUtf16: 12,
        endOffsetUtf16: 24,
        status: "open",
        version: 1,
        createdBy: "user-1",
        createdAt: "2026-02-07T00:00:00.000Z",
        resolvedAt: null,
      },
      message: {
        id: "msg-1",
        threadId: "thread-1",
        author: "Alice",
        bodyMd: "Looks good",
        createdAt: "2026-02-07T00:00:00.000Z",
        editedAt: null,
      },
    });
  });

  it("sets If-Match header when etag is provided for PATCH", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(200, {
        document: {
          id: "doc-1",
          workspace_id: "workspace-1",
          path: "docs/a.md",
          title: "Updated",
          tags: [],
          head_seq: 11,
          etag: "etag-new",
          archived_at: null,
          deleted_at: null,
          created_at: "2026-02-07T00:00:00.000Z",
          updated_at: "2026-02-07T00:10:00.000Z",
        },
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
    });

    await client.updateDocument(
      "workspace-1",
      "doc-1",
      { title: "Updated" },
      { etag: '"etag-old"' },
    );

    const [, init] = fetchMock.mock.calls[0]!;
    expect(init?.method).toBe("PATCH");
    expect(headerValue(init, "If-Match")).toBe('"etag-old"');
    expect(headerValue(init, "Idempotency-Key")).toBeNull();
  });

  it("exposes createWorkspace parity method from shared client", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(201, {
        workspace: {
          id: "w-new",
          slug: "alpha",
          name: "Alpha",
          role: "owner",
          createdAt: "2026-02-09T00:00:00.000Z",
          updatedAt: "2026-02-09T00:00:00.000Z",
          etag: "etag-w-new",
        },
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
      createIdempotencyKey: () => "idem-workspace",
    });

    const payload = await client.createWorkspace({
      name: "Alpha",
      slug: "alpha",
    });

    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://relay.scriptum.dev/v1/workspaces");
    expect(init?.method).toBe("POST");
    expect(headerValue(init, "Idempotency-Key")).toBe("idem-workspace");
    expect(init?.body).toBe(JSON.stringify({ name: "Alpha", slug: "alpha" }));
    expect(payload).toEqual({
      workspace: {
        id: "w-new",
        slug: "alpha",
        name: "Alpha",
        role: "owner",
        createdAt: "2026-02-09T00:00:00.000Z",
        updatedAt: "2026-02-09T00:00:00.000Z",
        etag: "etag-w-new",
      },
    });
  });

  it("exposes searchDocuments parity method from shared client", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(200, {
        items: [
          {
            doc_id: "doc-1",
            path: "docs/alpha.md",
            title: "Alpha",
            snippet: "alpha snippet",
            score: 0.98,
          },
        ],
        next_cursor: null,
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
    });

    const payload = await client.searchDocuments("workspace-1", {
      q: "alpha",
      limit: 5,
      cursor: "cursor-1",
    });

    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe(
      "https://relay.scriptum.dev/v1/workspaces/workspace-1/search?q=alpha&limit=5&cursor=cursor-1",
    );
    expect(init?.method).toBe("GET");
    expect(payload).toEqual({
      items: [
        {
          doc_id: "doc-1",
          path: "docs/alpha.md",
          title: "Alpha",
          snippet: "alpha snippet",
          score: 0.98,
        },
      ],
      next_cursor: null,
    });
  });

  it("posts threaded comment replies with body_md payload", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(201, {
        message: {
          id: "msg-2",
          thread_id: "thread-1",
          author_user_id: "user-1",
          body_md: "Follow-up",
          created_at: "2026-02-07T00:01:00.000Z",
          edited_at: null,
        },
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
      createIdempotencyKey: () => "idem-456",
    });

    const message = await client.addCommentMessage(
      "workspace-1",
      "thread-1",
      "Follow-up",
    );

    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe(
      "https://relay.scriptum.dev/v1/workspaces/workspace-1/comments/thread-1/messages",
    );
    expect(init?.method).toBe("POST");
    expect(headerValue(init, "Idempotency-Key")).toBe("idem-456");
    expect(init?.body).toBe(JSON.stringify({ body_md: "Follow-up" }));
    expect(message).toEqual({
      id: "msg-2",
      threadId: "thread-1",
      author: "user-1",
      bodyMd: "Follow-up",
      createdAt: "2026-02-07T00:01:00.000Z",
      editedAt: null,
    });
  });

  it("posts resolve and reopen transitions with if_version", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        jsonResponse(200, {
          thread: {
            id: "thread-1",
            doc_id: "doc-1",
            section_id: null,
            start_offset_utf16: 12,
            end_offset_utf16: 24,
            status: "resolved",
            version: 2,
            created_by_user_id: "user-1",
            created_at: "2026-02-07T00:00:00.000Z",
            resolved_at: "2026-02-07T00:02:00.000Z",
          },
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse(200, {
          thread: {
            id: "thread-1",
            doc_id: "doc-1",
            section_id: null,
            start_offset_utf16: 12,
            end_offset_utf16: 24,
            status: "open",
            version: 3,
            created_by_user_id: "user-1",
            created_at: "2026-02-07T00:00:00.000Z",
            resolved_at: null,
          },
        }),
      );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
      createIdempotencyKey: () => "idem-789",
    });

    const resolved = await client.resolveCommentThread(
      "workspace-1",
      "thread-1",
      1,
    );
    const reopened = await client.reopenCommentThread(
      "workspace-1",
      "thread-1",
      2,
    );

    const [resolveUrl, resolveInit] = fetchMock.mock.calls[0]!;
    expect(resolveUrl).toBe(
      "https://relay.scriptum.dev/v1/workspaces/workspace-1/comments/thread-1/resolve",
    );
    expect(resolveInit?.method).toBe("POST");
    expect(resolveInit?.body).toBe(JSON.stringify({ if_version: 1 }));
    expect(headerValue(resolveInit, "Idempotency-Key")).toBe("idem-789");

    const [reopenUrl, reopenInit] = fetchMock.mock.calls[1]!;
    expect(reopenUrl).toBe(
      "https://relay.scriptum.dev/v1/workspaces/workspace-1/comments/thread-1/reopen",
    );
    expect(reopenInit?.method).toBe("POST");
    expect(reopenInit?.body).toBe(JSON.stringify({ if_version: 2 }));
    expect(headerValue(reopenInit, "Idempotency-Key")).toBe("idem-789");

    expect(resolved).toMatchObject({
      id: "thread-1",
      status: "resolved",
      version: 2,
      createdBy: "user-1",
    });
    expect(reopened).toMatchObject({
      id: "thread-1",
      status: "open",
      version: 3,
      createdBy: "user-1",
    });
  });

  it("retries transient retryable responses", async () => {
    vi.useFakeTimers();
    const randomSpy = vi.spyOn(Math, "random").mockReturnValue(0);

    try {
      const fetchMock = vi
        .fn<typeof fetch>()
        .mockResolvedValueOnce(
          jsonResponse(503, {
            error: {
              code: "TEMP",
              message: "temporary outage",
              retryable: true,
            },
          }),
        )
        .mockResolvedValueOnce(
          jsonResponse(200, {
            items: [
              {
                id: "w1",
                slug: "alpha",
                name: "Alpha",
                role: "editor",
                created_at: "2026-02-07T00:00:00.000Z",
                updated_at: "2026-02-07T00:01:00.000Z",
                etag: "etag-w1",
              },
            ],
            next_cursor: null,
          }),
        );

      const client = createApiClient({
        baseUrl: "https://relay.scriptum.dev",
        fetch: fetchMock,
        getAccessToken: async () => null,
        maxRetries: 2,
      });

      const request = client.listWorkspaces();
      await vi.runAllTimersAsync();
      await expect(request).resolves.toEqual({
        items: [
          {
            id: "w1",
            slug: "alpha",
            name: "Alpha",
            role: "editor",
            createdAt: "2026-02-07T00:00:00.000Z",
            updatedAt: "2026-02-07T00:01:00.000Z",
            etag: "etag-w1",
          },
        ],
        nextCursor: null,
      });
      expect(fetchMock).toHaveBeenCalledTimes(2);
    } finally {
      randomSpy.mockRestore();
      vi.useRealTimers();
    }
  });

  it("parses relay error envelope into ApiClientError", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(409, {
        error: {
          code: "DOC_PATH_CONFLICT",
          message: "Document path already exists",
          retryable: false,
          request_id: "req-123",
          details: { path: "docs/auth.md" },
        },
      }),
    );

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
    });

    await expect(client.listWorkspaces()).rejects.toMatchObject({
      name: "ApiClientError",
      status: 409,
      method: "GET",
      url: "https://relay.scriptum.dev/v1/workspaces",
      code: "DOC_PATH_CONFLICT",
      message: "Document path already exists",
      retryable: false,
      requestId: "req-123",
      details: { path: "docs/auth.md" },
    } satisfies Partial<ApiClientError>);
  });

  it("uses default relay base URL when baseUrl is not provided", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(jsonResponse(200, { items: [], next_cursor: null }));

    const client = createApiClient({
      fetch: fetchMock,
      getAccessToken: async () => null,
    });

    await client.listWorkspaces();
    const [url] = fetchMock.mock.calls[0]!;
    expect(url).toBe("http://localhost:8080/v1/workspaces");
  });

  it("omits Authorization header when access token is unavailable", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(jsonResponse(200, { items: [], next_cursor: null }));

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => null,
    });

    await client.listWorkspaces();
    const [, init] = fetchMock.mock.calls[0]!;
    expect(headerValue(init, "Authorization")).toBeNull();
  });

  it("propagates AbortError from cancelled requests", async () => {
    const abortError = new DOMException(
      "The operation was aborted",
      "AbortError",
    );
    const fetchMock = vi.fn<typeof fetch>().mockRejectedValue(abortError);

    const client = createApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetch: fetchMock,
      getAccessToken: async () => "token-1",
    });

    await expect(client.listWorkspaces()).rejects.toBe(abortError);
  });
});
