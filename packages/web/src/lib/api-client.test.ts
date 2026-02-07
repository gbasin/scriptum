import { describe, expect, it, vi } from "vitest";
import {
  ApiClientError,
  createApiClient,
  type CreateCommentInput,
} from "./api-client";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function headerValue(init: RequestInit | undefined, key: string): string | null {
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

    const response = await client.listWorkspaces({ limit: 10, cursor: "cur-1" });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;

    expect(url).toBe("https://relay.scriptum.dev/v1/workspaces?limit=10&cursor=cur-1");
    expect(init?.method).toBe("GET");
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

    const response = await client.createComment("workspace-1", "doc-1", input);

    const [, init] = fetchMock.mock.calls[0]!;
    expect(init?.method).toBe("POST");
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
      { etag: "\"etag-old\"" },
    );

    const [, init] = fetchMock.mock.calls[0]!;
    expect(init?.method).toBe("PATCH");
    expect(headerValue(init, "If-Match")).toBe("\"etag-old\"");
    expect(headerValue(init, "Idempotency-Key")).toBeNull();
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
});
