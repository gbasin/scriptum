// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthService, AuthSession } from "./auth";

// --- Mocks for @tauri-apps/api ---

const mockInvoke = vi.fn();
const mockListen = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => mockListen(...args),
}));

// Import after mocks are set up
const { isTauri } = await import("./tauri-auth");

function makeSession(overrides: Partial<AuthSession> = {}): AuthSession {
  return {
    accessToken: "access-token-1",
    accessExpiresAt: "2026-02-08T01:00:00.000Z",
    refreshToken: "refresh-token-1",
    refreshExpiresAt: "2026-02-15T00:00:00.000Z",
    user: {
      id: "user-1",
      email: "test@example.com",
      display_name: "Test User",
    },
    ...overrides,
  };
}

type MockAuth = {
  [K in keyof AuthService]: ReturnType<typeof vi.fn>;
};

function makeMockAuth(): MockAuth {
  return {
    startGitHubOAuth: vi.fn(),
    handleOAuthCallback: vi.fn(),
    getStoredSession: vi.fn(),
    refreshAccessToken: vi.fn(),
    getAccessToken: vi.fn(),
    logout: vi.fn(),
    clearStoredSession: vi.fn(),
  };
}

describe("isTauri()", () => {
  const original = (window as unknown as Record<string, unknown>)
    .__TAURI_INTERNALS__;

  afterEach(() => {
    if (original === undefined) {
      delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    } else {
      (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ =
        original;
    }
  });

  it("returns false when __TAURI_INTERNALS__ is absent", () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    expect(isTauri()).toBe(false);
  });

  it("returns true when __TAURI_INTERNALS__ is present", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(isTauri()).toBe(true);
  });
});

describe("performTauriLogin", () => {
  let performTauriLogin: typeof import("./tauri-auth").performTauriLogin;

  beforeEach(async () => {
    vi.clearAllMocks();
    const mod = await import("./tauri-auth");
    performTauriLogin = mod.performTauriLogin;
  });

  it("happy path: start → open browser → deep link → exchange → session", async () => {
    const session = makeSession();
    const auth = makeMockAuth();
    let deepLinkHandler: ((event: { payload: string[] }) => void) | null = null;

    mockInvoke.mockImplementation(
      (command: string, args?: Record<string, unknown>) => {
        if (command === "auth_redirect_uri") {
          return Promise.resolve("scriptum://auth/callback");
        }
        if (command === "auth_open_browser") {
          // Simulate the user completing auth — fire deep link after browser opens
          setTimeout(() => {
            deepLinkHandler?.({
              payload: [
                "scriptum://auth/callback?code=gh-code-1&state=state-uuid-1",
              ],
            });
          }, 10);
          return Promise.resolve();
        }
        if (command === "auth_parse_callback") {
          return Promise.resolve({
            url: (args as Record<string, string>).url,
            code: "gh-code-1",
            state: "state-uuid-1",
            error: null,
            error_description: null,
          });
        }
        if (command === "auth_store_tokens") {
          return Promise.resolve();
        }
        return Promise.reject(new Error(`Unexpected command: ${command}`));
      },
    );

    mockListen.mockImplementation(
      (_event: string, handler: (event: { payload: string[] }) => void) => {
        deepLinkHandler = handler;
        return Promise.resolve(() => {});
      },
    );

    auth.startGitHubOAuth.mockResolvedValue({
      flowId: "flow-1",
      authorizationUrl: "https://github.com/login/oauth/authorize?...",
      expiresAt: "2026-02-08T01:00:00.000Z",
      state: "state-uuid-1",
    });
    auth.handleOAuthCallback.mockResolvedValue(session);

    const result = await performTauriLogin(
      auth as unknown as Pick<
        AuthService,
        "startGitHubOAuth" | "handleOAuthCallback"
      >,
    );

    expect(auth.startGitHubOAuth).toHaveBeenCalledWith({
      redirectUri: "scriptum://auth/callback",
      redirect: false,
    });

    expect(mockInvoke).toHaveBeenCalledWith("auth_open_browser", {
      authorizationUrl: "https://github.com/login/oauth/authorize?...",
    });

    expect(auth.handleOAuthCallback).toHaveBeenCalledWith({
      searchParams: expect.any(URLSearchParams),
    });
    const params = auth.handleOAuthCallback.mock.calls[0][0].searchParams;
    expect(params.get("code")).toBe("gh-code-1");
    expect(params.get("state")).toBe("state-uuid-1");

    expect(mockInvoke).toHaveBeenCalledWith("auth_store_tokens", {
      tokens: {
        access_token: session.accessToken,
        refresh_token: session.refreshToken,
        access_expires_at: session.accessExpiresAt,
        refresh_expires_at: session.refreshExpiresAt,
      },
    });

    expect(result).toEqual(session);
  });

  it("propagates startGitHubOAuth errors and cancels listener", async () => {
    const auth = makeMockAuth();
    const unlistenSpy = vi.fn();

    mockInvoke.mockImplementation((command: string) => {
      if (command === "auth_redirect_uri") {
        return Promise.resolve("scriptum://auth/callback");
      }
      return Promise.reject(new Error(`Unexpected: ${command}`));
    });

    mockListen.mockResolvedValue(unlistenSpy);

    auth.startGitHubOAuth.mockRejectedValue(new Error("Relay down"));

    await expect(
      performTauriLogin(
        auth as unknown as Pick<
          AuthService,
          "startGitHubOAuth" | "handleOAuthCallback"
        >,
      ),
    ).rejects.toThrow("Relay down");
  });

  it("propagates deep link errors", async () => {
    const auth = makeMockAuth();
    let deepLinkHandler: ((event: { payload: string[] }) => void) | null = null;

    mockInvoke.mockImplementation((command: string) => {
      if (command === "auth_redirect_uri") {
        return Promise.resolve("scriptum://auth/callback");
      }
      if (command === "auth_open_browser") {
        setTimeout(() => {
          deepLinkHandler?.({
            payload: [
              "scriptum://auth/callback?error=access_denied&error_description=User+denied",
            ],
          });
        }, 10);
        return Promise.resolve();
      }
      if (command === "auth_parse_callback") {
        return Promise.resolve({
          url: "",
          code: null,
          state: null,
          error: "access_denied",
          error_description: "User denied",
        });
      }
      return Promise.reject(new Error(`Unexpected: ${command}`));
    });

    mockListen.mockImplementation(
      (_event: string, handler: (event: { payload: string[] }) => void) => {
        deepLinkHandler = handler;
        return Promise.resolve(() => {});
      },
    );

    auth.startGitHubOAuth.mockResolvedValue({
      flowId: "flow-1",
      authorizationUrl: "https://github.com/login/oauth/authorize?...",
      expiresAt: "2026-02-08T01:00:00.000Z",
      state: "state-uuid-1",
    });

    await expect(
      performTauriLogin(
        auth as unknown as Pick<
          AuthService,
          "startGitHubOAuth" | "handleOAuthCallback"
        >,
      ),
    ).rejects.toThrow("User denied");
  });
});

describe("listenForDeepLinkCallback", () => {
  let listenForDeepLinkCallback: typeof import("./tauri-auth").listenForDeepLinkCallback;

  beforeEach(async () => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    const mod = await import("./tauri-auth");
    listenForDeepLinkCallback = mod.listenForDeepLinkCallback;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("rejects on timeout", async () => {
    mockListen.mockResolvedValue(() => {});
    mockInvoke.mockResolvedValue(undefined);

    const listener = listenForDeepLinkCallback({ timeoutMs: 5_000 });

    vi.advanceTimersByTime(5_001);

    await expect(listener.promise).rejects.toThrow(
      "Deep link callback timed out",
    );
  });
});
