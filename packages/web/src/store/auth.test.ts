import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthUser, OAuthCallbackResult, RefreshResult } from "../auth/client";
import { AuthClient, AuthClientError } from "../auth/client";
import * as storage from "../auth/storage";
import { installMockLocalStorage } from "../test/mock-local-storage";
import { createAuthStore } from "./auth";

// ── Mocks ──────────────────────────────────────────────────────────────

vi.mock("../auth/pkce", () => ({
  generateCodeVerifier: () => "test-verifier-abc123",
  generateCodeChallenge: () => Promise.resolve("test-challenge-xyz"),
}));

const MOCK_USER: AuthUser = {
  id: "user-1",
  email: "alice@example.com",
  display_name: "Alice",
};

const MOCK_ACCESS_EXPIRES = "2099-01-01T01:00:00Z";
const MOCK_REFRESH_EXPIRES = "2099-02-01T00:00:00Z";

function mockSession(): storage.StoredSession {
  return {
    accessToken: "access-tok-1",
    accessExpiresAt: MOCK_ACCESS_EXPIRES,
    refreshToken: "refresh-tok-1",
    refreshExpiresAt: MOCK_REFRESH_EXPIRES,
    user: MOCK_USER,
  };
}

// ── Tests ──────────────────────────────────────────────────────────────

describe("auth store", () => {
  let cleanupStorage: () => void;

  beforeEach(() => {
    cleanupStorage = installMockLocalStorage();
    vi.spyOn(crypto, "randomUUID").mockReturnValue(
      "state-uuid-1234" as ReturnType<typeof crypto.randomUUID>,
    );
  });

  afterEach(() => {
    cleanupStorage();
    vi.restoreAllMocks();
  });

  it("starts with unknown status", () => {
    const store = createAuthStore();
    expect(store.getState().status).toBe("unknown");
    expect(store.getState().user).toBeNull();
    expect(store.getState().accessToken).toBeNull();
  });

  // ── restoreSession ─────────────────────────────────────────────────

  it("restoreSession: sets unauthenticated when no stored session", () => {
    const store = createAuthStore();
    store.getState().restoreSession();
    expect(store.getState().status).toBe("unauthenticated");
  });

  it("restoreSession: loads valid session from localStorage", () => {
    const session = mockSession();
    storage.saveSession(session);

    const store = createAuthStore();
    store.getState().restoreSession();

    expect(store.getState().status).toBe("authenticated");
    expect(store.getState().user).toEqual(MOCK_USER);
    expect(store.getState().accessToken).toBe("access-tok-1");
    expect(store.getState().refreshToken).toBe("refresh-tok-1");
  });

  it("restoreSession: clears expired refresh token", () => {
    const session = mockSession();
    session.refreshExpiresAt = "2020-01-01T00:00:00Z"; // expired
    storage.saveSession(session);

    const store = createAuthStore();
    store.getState().restoreSession();

    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().user).toBeNull();
  });

  // ── startLogin ─────────────────────────────────────────────────────

  it("startLogin: calls startOAuth and saves flow data", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "startOAuth").mockResolvedValue({
      flow_id: "flow-42",
      authorization_url: "https://github.com/login/oauth/authorize?...",
      expires_at: "2099-01-01T00:00:00Z",
    });

    // Prevent navigation error by stubbing window.location.href.
    const origHref = Object.getOwnPropertyDescriptor(globalThis, "location");
    Object.defineProperty(globalThis, "location", {
      value: { href: "" },
      writable: true,
      configurable: true,
    });

    const store = createAuthStore();
    await store.getState().startLogin(client, "http://localhost:3000/auth-callback");

    // Restore.
    if (origHref) {
      Object.defineProperty(globalThis, "location", origHref);
    }

    expect(client.startOAuth).toHaveBeenCalledWith({
      redirect_uri: "http://localhost:3000/auth-callback",
      state: "state-uuid-1234",
      code_challenge: "test-challenge-xyz",
      code_challenge_method: "S256",
    });

    const flow = storage.loadOAuthFlow();
    expect(flow).toEqual({
      state: "state-uuid-1234",
      codeVerifier: "test-verifier-abc123",
      flowId: "flow-42",
    });
  });

  it("startLogin: sets error on failure", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "startOAuth").mockRejectedValue(
      new AuthClientError(429, "rate limited"),
    );

    const store = createAuthStore();
    await store.getState().startLogin(client, "http://localhost:3000/auth-callback");

    expect(store.getState().error).toBe("Login failed (429)");
  });

  // ── handleCallback ─────────────────────────────────────────────────

  it("handleCallback: exchanges code and stores session", async () => {
    storage.saveOAuthFlow({
      state: "callback-state",
      codeVerifier: "verifier-1",
      flowId: "flow-99",
    });

    const callbackResult: OAuthCallbackResult = {
      access_token: "at-new",
      access_expires_at: MOCK_ACCESS_EXPIRES,
      refresh_token: "rt-new",
      refresh_expires_at: MOCK_REFRESH_EXPIRES,
      user: MOCK_USER,
    };
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "exchangeCode").mockResolvedValue(callbackResult);

    const store = createAuthStore();
    await store.getState().handleCallback(client, "github-code", "callback-state");

    expect(store.getState().status).toBe("authenticated");
    expect(store.getState().accessToken).toBe("at-new");
    expect(store.getState().user).toEqual(MOCK_USER);
    expect(store.getState().error).toBeNull();

    // Flow data should be cleared.
    expect(storage.loadOAuthFlow()).toBeNull();
  });

  it("handleCallback: rejects state mismatch", async () => {
    storage.saveOAuthFlow({
      state: "expected-state",
      codeVerifier: "v",
      flowId: "f",
    });

    const client = new AuthClient({ baseUrl: "http://relay" });
    const store = createAuthStore();
    await store.getState().handleCallback(client, "code", "wrong-state");

    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().error).toContain("state mismatch");
  });

  it("handleCallback: handles missing flow data", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    const store = createAuthStore();
    await store.getState().handleCallback(client, "code", "state");

    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().error).toContain("not found");
  });

  it("handleCallback: handles API error", async () => {
    storage.saveOAuthFlow({
      state: "s",
      codeVerifier: "v",
      flowId: "f",
    });

    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "exchangeCode").mockRejectedValue(
      new AuthClientError(401, "invalid code"),
    );

    const store = createAuthStore();
    await store.getState().handleCallback(client, "bad-code", "s");

    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().error).toBe("Authentication failed (401)");
  });

  // ── refreshAccessToken ─────────────────────────────────────────────

  it("refreshAccessToken: rotates tokens on success", async () => {
    const refreshResult: RefreshResult = {
      access_token: "at-refreshed",
      access_expires_at: MOCK_ACCESS_EXPIRES,
      refresh_token: "rt-rotated",
      refresh_expires_at: MOCK_REFRESH_EXPIRES,
    };
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "refreshToken").mockResolvedValue(refreshResult);

    const store = createAuthStore({
      status: "authenticated",
      user: MOCK_USER,
      accessToken: "at-old",
      accessExpiresAt: "2020-01-01T00:00:00Z",
      refreshToken: "rt-old",
      refreshExpiresAt: MOCK_REFRESH_EXPIRES,
    });

    const ok = await store.getState().refreshAccessToken(client);
    expect(ok).toBe(true);
    expect(store.getState().accessToken).toBe("at-refreshed");
    expect(store.getState().refreshToken).toBe("rt-rotated");
  });

  it("refreshAccessToken: returns false with no refresh token", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    const store = createAuthStore({ status: "authenticated", refreshToken: null });

    const ok = await store.getState().refreshAccessToken(client);
    expect(ok).toBe(false);
    expect(store.getState().status).toBe("unauthenticated");
  });

  it("refreshAccessToken: sets unauthenticated on failure", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "refreshToken").mockRejectedValue(
      new AuthClientError(401, "revoked"),
    );

    const store = createAuthStore({
      status: "authenticated",
      user: MOCK_USER,
      refreshToken: "rt-bad",
    });

    const ok = await store.getState().refreshAccessToken(client);
    expect(ok).toBe(false);
    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().error).toContain("expired");
  });

  // ── logout ─────────────────────────────────────────────────────────

  it("logout: revokes session and clears state", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "logout").mockResolvedValue(undefined);

    const store = createAuthStore({
      status: "authenticated",
      user: MOCK_USER,
      accessToken: "at",
      refreshToken: "rt",
    });

    await store.getState().logout(client);

    expect(client.logout).toHaveBeenCalledWith("rt");
    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().user).toBeNull();
    expect(store.getState().accessToken).toBeNull();
  });

  it("logout: clears state even if server revocation fails", async () => {
    const client = new AuthClient({ baseUrl: "http://relay" });
    vi.spyOn(client, "logout").mockRejectedValue(new Error("network"));

    const store = createAuthStore({
      status: "authenticated",
      refreshToken: "rt",
    });

    await store.getState().logout(client);
    expect(store.getState().status).toBe("unauthenticated");
  });

  // ── isAccessTokenExpired ───────────────────────────────────────────

  it("isAccessTokenExpired: true when no expiry", () => {
    const store = createAuthStore();
    expect(store.getState().isAccessTokenExpired()).toBe(true);
  });

  it("isAccessTokenExpired: false for future expiry", () => {
    const store = createAuthStore({ accessExpiresAt: MOCK_ACCESS_EXPIRES });
    expect(store.getState().isAccessTokenExpired()).toBe(false);
  });

  it("isAccessTokenExpired: true for past expiry", () => {
    const store = createAuthStore({ accessExpiresAt: "2020-01-01T00:00:00Z" });
    expect(store.getState().isAccessTokenExpired()).toBe(true);
  });

  it("isAccessTokenExpired: respects buffer", () => {
    const soon = new Date(Date.now() + 30_000).toISOString();
    const store = createAuthStore({ accessExpiresAt: soon });
    expect(store.getState().isAccessTokenExpired(60_000)).toBe(true);
    expect(store.getState().isAccessTokenExpired(10_000)).toBe(false);
  });

  // ── reset ──────────────────────────────────────────────────────────

  it("resets to initial state", () => {
    const store = createAuthStore({
      status: "authenticated",
      user: MOCK_USER,
      accessToken: "at",
    });

    store.getState().reset();
    expect(store.getState().status).toBe("unknown");
    expect(store.getState().user).toBeNull();
  });
});
