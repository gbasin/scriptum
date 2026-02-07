import { describe, expect, it, vi } from "vitest";
import { ScriptumApiClient, ScriptumApiError } from "./client";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function readHeader(init: RequestInit | undefined, key: string): string | null {
  return new Headers(init?.headers).get(key);
}

describe("ScriptumApiClient", () => {
  it("injects bearer auth header from token provider", async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(200, {
        items: [],
        next_cursor: null,
      }),
    );

    const client = new ScriptumApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetchImpl,
      tokenProvider: () => "token-123",
    });

    await client.listWorkspaces({ limit: 10 });

    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe("https://relay.scriptum.dev/v1/workspaces?limit=10");
    expect(readHeader(init, "Authorization")).toBe("Bearer token-123");
  });

  it("adds idempotency key for non-auth POST and supports If-Match for PATCH", async () => {
    const fetchImpl = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        jsonResponse(201, {
          workspace: {
            id: "w-1",
            slug: "w-1",
            name: "Workspace",
            role: "owner",
            createdAt: "2026-02-07T00:00:00.000Z",
            updatedAt: "2026-02-07T00:00:00.000Z",
            etag: "etag-1",
          },
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse(200, {
          workspace: {
            id: "w-1",
            slug: "w-1-updated",
            name: "Workspace",
            role: "owner",
            createdAt: "2026-02-07T00:00:00.000Z",
            updatedAt: "2026-02-07T00:01:00.000Z",
            etag: "etag-2",
          },
        }),
      );

    const client = new ScriptumApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetchImpl,
      tokenProvider: () => "token-xyz",
      idempotencyKeyFactory: () => "idem-1",
    });

    await client.createWorkspace({ name: "Workspace", slug: "w-1" });
    await client.updateWorkspace(
      "w-1",
      { slug: "w-1-updated" },
      { ifMatch: "\"etag-1\"" },
    );

    const [, createInit] = fetchImpl.mock.calls[0]!;
    expect(createInit?.method).toBe("POST");
    expect(readHeader(createInit, "Idempotency-Key")).toBe("idem-1");

    const [, patchInit] = fetchImpl.mock.calls[1]!;
    expect(patchInit?.method).toBe("PATCH");
    expect(readHeader(patchInit, "If-Match")).toBe("\"etag-1\"");
  });

  it("does not add idempotency key to auth endpoints", async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(200, {
        flow_id: "flow-1",
        authorization_url: "https://github.com/login/oauth/authorize",
        expires_at: "2026-02-07T00:10:00.000Z",
      }),
    );

    const client = new ScriptumApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetchImpl,
      tokenProvider: () => null,
      idempotencyKeyFactory: () => "idem-auth",
    });

    await client.authOAuthStart({
      redirect_uri: "https://app.scriptum.dev/auth-callback",
      state: "state-1",
      code_challenge: "challenge",
      code_challenge_method: "S256",
    });

    const [, init] = fetchImpl.mock.calls[0]!;
    expect(init?.method).toBe("POST");
    expect(readHeader(init, "Idempotency-Key")).toBeNull();
  });

  it("parses relay error envelope and throws ScriptumApiError", async () => {
    const fetchImpl = vi.fn<typeof fetch>().mockResolvedValue(
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

    const client = new ScriptumApiClient({
      baseUrl: "https://relay.scriptum.dev",
      fetchImpl,
      tokenProvider: () => "token-1",
    });

    await expect(client.listWorkspaces()).rejects.toMatchObject({
      name: "ScriptumApiError",
      status: 409,
      method: "GET",
      url: "https://relay.scriptum.dev/v1/workspaces",
      code: "DOC_PATH_CONFLICT",
      message: "Document path already exists",
      retryable: false,
      requestId: "req-123",
      details: { path: "docs/auth.md" },
    } satisfies Partial<ScriptumApiError>);
  });
});
