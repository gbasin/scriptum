import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockLocalStorage } from "../test/mock-local-storage";
import type { AuthApi, AuthSession } from "./auth";
import { type AuthFlowError, createAuthService } from "./auth";

vi.mock("../auth/pkce", () => ({
  generateCodeVerifier: () => "test-code-verifier",
  generateCodeChallenge: () => Promise.resolve("test-code-challenge"),
}));

type MockClient = AuthApi & {
  startOAuth: ReturnType<typeof vi.fn>;
  exchangeCode: ReturnType<typeof vi.fn>;
  refreshToken: ReturnType<typeof vi.fn>;
  logout: ReturnType<typeof vi.fn>;
};

function makeMockClient(): MockClient {
  return {
    startOAuth: vi.fn(),
    exchangeCode: vi.fn(),
    refreshToken: vi.fn(),
    logout: vi.fn(),
  } as MockClient;
}

function setStoredSession(session: AuthSession): void {
  localStorage.setItem("scriptum:access_token", session.accessToken);
  localStorage.setItem("scriptum:access_expires_at", session.accessExpiresAt);
  localStorage.setItem("scriptum:refresh_token", session.refreshToken);
  localStorage.setItem("scriptum:refresh_expires_at", session.refreshExpiresAt);
  localStorage.setItem("scriptum:user", JSON.stringify(session.user));
}

describe("lib/auth", () => {
  const baseNow = Date.parse("2026-02-07T00:00:00.000Z");

  let originalLocalStorage: Storage;
  let originalSessionStorage: Storage;

  beforeEach(() => {
    originalLocalStorage = globalThis.localStorage;
    originalSessionStorage = globalThis.sessionStorage;

    Object.defineProperty(globalThis, "localStorage", {
      value: new MockLocalStorage(),
      writable: true,
      configurable: true,
    });
    Object.defineProperty(globalThis, "sessionStorage", {
      value: new MockLocalStorage(),
      writable: true,
      configurable: true,
    });

    vi.spyOn(crypto, "randomUUID").mockReturnValue(
      "state-uuid-1" as ReturnType<typeof crypto.randomUUID>,
    );
  });

  afterEach(() => {
    Object.defineProperty(globalThis, "localStorage", {
      value: originalLocalStorage,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(globalThis, "sessionStorage", {
      value: originalSessionStorage,
      writable: true,
      configurable: true,
    });
    vi.restoreAllMocks();
  });

  it("startGitHubOAuth stores flow data in sessionStorage", async () => {
    const client = makeMockClient();
    client.startOAuth.mockResolvedValue({
      flow_id: "flow-1",
      authorization_url: "https://github.com/oauth/authorize?flow=1",
      expires_at: "2026-02-07T01:00:00.000Z",
    });

    const location = {
      assign: vi.fn(),
      origin: "https://app.scriptum.dev",
    };

    const auth = createAuthService({
      client,
      location,
      now: () => baseNow,
    });

    const result = await auth.startGitHubOAuth({ redirect: false });

    expect(client.startOAuth).toHaveBeenCalledWith({
      redirect_uri: "https://app.scriptum.dev/auth-callback",
      state: "state-uuid-1",
      code_challenge: "test-code-challenge",
      code_challenge_method: "S256",
    });
    expect(result).toEqual({
      flowId: "flow-1",
      authorizationUrl: "https://github.com/oauth/authorize?flow=1",
      expiresAt: "2026-02-07T01:00:00.000Z",
      state: "state-uuid-1",
    });

    expect(sessionStorage.getItem("scriptum:oauth_state")).toBe("state-uuid-1");
    expect(sessionStorage.getItem("scriptum:code_verifier")).toBe(
      "test-code-verifier",
    );
    expect(sessionStorage.getItem("scriptum:flow_id")).toBe("flow-1");
    expect(location.assign).not.toHaveBeenCalled();
  });

  it("startGitHubOAuth redirects to authorization URL by default", async () => {
    const client = makeMockClient();
    client.startOAuth.mockResolvedValue({
      flow_id: "flow-2",
      authorization_url: "https://github.com/oauth/authorize?flow=2",
      expires_at: "2026-02-07T01:00:00.000Z",
    });

    const location = {
      assign: vi.fn(),
      origin: "https://app.scriptum.dev",
    };

    const auth = createAuthService({
      client,
      location,
      now: () => baseNow,
    });

    await auth.startGitHubOAuth();
    expect(location.assign).toHaveBeenCalledWith(
      "https://github.com/oauth/authorize?flow=2",
    );
  });

  it("handleOAuthCallback exchanges code and persists tokens", async () => {
    sessionStorage.setItem("scriptum:oauth_state", "state-1");
    sessionStorage.setItem("scriptum:code_verifier", "verifier-1");
    sessionStorage.setItem("scriptum:flow_id", "flow-1");

    const client = makeMockClient();
    client.exchangeCode.mockResolvedValue({
      access_token: "at-1",
      access_expires_at: "2026-02-07T00:30:00.000Z",
      refresh_token: "rt-1",
      refresh_expires_at: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    const session = await auth.handleOAuthCallback({
      searchParams: new URLSearchParams("code=gh-code&state=state-1"),
    });

    expect(client.exchangeCode).toHaveBeenCalledWith({
      flow_id: "flow-1",
      code: "gh-code",
      state: "state-1",
      code_verifier: "verifier-1",
    });
    expect(session.accessToken).toBe("at-1");
    expect(session.refreshToken).toBe("rt-1");
    expect(auth.getStoredSession()).toEqual(session);
    expect(sessionStorage.getItem("scriptum:oauth_state")).toBeNull();
    expect(sessionStorage.getItem("scriptum:code_verifier")).toBeNull();
    expect(sessionStorage.getItem("scriptum:flow_id")).toBeNull();
  });

  it("handleOAuthCallback rejects a state mismatch", async () => {
    sessionStorage.setItem("scriptum:oauth_state", "expected");
    sessionStorage.setItem("scriptum:code_verifier", "verifier");
    sessionStorage.setItem("scriptum:flow_id", "flow");

    const client = makeMockClient();
    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await expect(
      auth.handleOAuthCallback({
        searchParams: new URLSearchParams("code=abc&state=wrong"),
      }),
    ).rejects.toMatchObject({
      code: "OAUTH_STATE_MISMATCH",
    } satisfies Partial<AuthFlowError>);

    expect(client.exchangeCode).not.toHaveBeenCalled();
    expect(sessionStorage.getItem("scriptum:oauth_state")).toBeNull();
  });

  it("handleOAuthCallback rejects missing callback params", async () => {
    const client = makeMockClient();
    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await expect(
      auth.handleOAuthCallback({ searchParams: new URLSearchParams("state=s") }),
    ).rejects.toMatchObject({
      code: "OAUTH_CALLBACK_PARAMS_MISSING",
    } satisfies Partial<AuthFlowError>);
    expect(client.exchangeCode).not.toHaveBeenCalled();
  });

  it("handleOAuthCallback rejects when stored OAuth flow is missing", async () => {
    const client = makeMockClient();
    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await expect(
      auth.handleOAuthCallback({
        searchParams: new URLSearchParams("code=abc&state=s"),
      }),
    ).rejects.toMatchObject({
      code: "OAUTH_FLOW_MISSING",
    } satisfies Partial<AuthFlowError>);
    expect(client.exchangeCode).not.toHaveBeenCalled();
  });

  it("getAccessToken returns current token when not near expiry", async () => {
    setStoredSession({
      accessToken: "at-stable",
      accessExpiresAt: "2026-02-07T00:10:00.000Z",
      refreshToken: "rt-stable",
      refreshExpiresAt: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const client = makeMockClient();
    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await expect(auth.getAccessToken()).resolves.toBe("at-stable");
    expect(client.refreshToken).not.toHaveBeenCalled();
  });

  it("getAccessToken refreshes when access token is near expiry", async () => {
    setStoredSession({
      accessToken: "at-old",
      accessExpiresAt: "2026-02-07T00:00:30.000Z",
      refreshToken: "rt-old",
      refreshExpiresAt: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const client = makeMockClient();
    client.refreshToken.mockResolvedValue({
      access_token: "at-new",
      access_expires_at: "2026-02-07T00:30:00.000Z",
      refresh_token: "rt-new",
      refresh_expires_at: "2026-03-10T00:00:00.000Z",
    });

    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await expect(auth.getAccessToken()).resolves.toBe("at-new");
    expect(client.refreshToken).toHaveBeenCalledWith("rt-old");
    expect(auth.getStoredSession()).toEqual({
      accessToken: "at-new",
      accessExpiresAt: "2026-02-07T00:30:00.000Z",
      refreshToken: "rt-new",
      refreshExpiresAt: "2026-03-10T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });
  });

  it("getAccessToken clears session when refresh fails", async () => {
    setStoredSession({
      accessToken: "at-old",
      accessExpiresAt: "2026-02-07T00:00:30.000Z",
      refreshToken: "rt-old",
      refreshExpiresAt: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const client = makeMockClient();
    client.refreshToken.mockRejectedValue(new Error("refresh failed"));

    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await expect(auth.getAccessToken()).resolves.toBeNull();
    expect(auth.getStoredSession()).toBeNull();
  });

  it("deduplicates concurrent refresh requests", async () => {
    setStoredSession({
      accessToken: "at-old",
      accessExpiresAt: "2026-02-07T00:00:30.000Z",
      refreshToken: "rt-old",
      refreshExpiresAt: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const client = makeMockClient();
    client.refreshToken.mockImplementation(async () => {
      await Promise.resolve();
      return {
        access_token: "at-new",
        access_expires_at: "2026-02-07T00:30:00.000Z",
        refresh_token: "rt-new",
        refresh_expires_at: "2026-03-10T00:00:00.000Z",
      };
    });

    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    const [tokenA, tokenB] = await Promise.all([
      auth.getAccessToken(),
      auth.getAccessToken(),
    ]);

    expect(tokenA).toBe("at-new");
    expect(tokenB).toBe("at-new");
    expect(client.refreshToken).toHaveBeenCalledTimes(1);
  });

  it("clears stored session when refresh token is expired", () => {
    setStoredSession({
      accessToken: "at-old",
      accessExpiresAt: "2026-02-07T00:30:00.000Z",
      refreshToken: "rt-old",
      refreshExpiresAt: "2026-01-01T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const auth = createAuthService({
      client: makeMockClient(),
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    expect(auth.getStoredSession()).toBeNull();
    expect(localStorage.getItem("scriptum:access_token")).toBeNull();
    expect(localStorage.getItem("scriptum:refresh_token")).toBeNull();
  });

  it("logout calls relay logout and clears local session", async () => {
    setStoredSession({
      accessToken: "at",
      accessExpiresAt: "2026-02-07T00:10:00.000Z",
      refreshToken: "rt",
      refreshExpiresAt: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });
    sessionStorage.setItem("scriptum:oauth_state", "state");

    const client = makeMockClient();
    client.logout.mockResolvedValue(undefined);

    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await auth.logout();

    expect(client.logout).toHaveBeenCalledWith("rt");
    expect(auth.getStoredSession()).toBeNull();
    expect(sessionStorage.getItem("scriptum:oauth_state")).toBeNull();
  });

  it("logout still clears local session when relay logout fails", async () => {
    setStoredSession({
      accessToken: "at",
      accessExpiresAt: "2026-02-07T00:10:00.000Z",
      refreshToken: "rt",
      refreshExpiresAt: "2026-03-07T00:00:00.000Z",
      user: { id: "u1", email: "alice@example.com", display_name: "Alice" },
    });

    const client = makeMockClient();
    client.logout.mockRejectedValue(new Error("relay unavailable"));

    const auth = createAuthService({
      client,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
      now: () => baseNow,
    });

    await auth.logout();
    expect(auth.getStoredSession()).toBeNull();
  });
});
