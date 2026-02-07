// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthService, AuthSession } from "../lib/auth";
import { type AuthStore, createAuthStore } from "../store/auth";
import { type UseAuthOptions, type UseAuthResult, useAuth } from "./useAuth";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

type MockAuth = {
  getStoredSession: ReturnType<typeof vi.fn>;
  startGitHubOAuth: ReturnType<typeof vi.fn>;
  refreshAccessToken: ReturnType<typeof vi.fn>;
  logout: ReturnType<typeof vi.fn>;
};

function createMockAuth(): MockAuth {
  return {
    getStoredSession: vi.fn(),
    startGitHubOAuth: vi.fn(),
    refreshAccessToken: vi.fn(),
    logout: vi.fn(),
  };
}

function authOptions(auth: MockAuth): UseAuthOptions["auth"] {
  return auth as unknown as Pick<
    AuthService,
    "getStoredSession" | "startGitHubOAuth" | "refreshAccessToken" | "logout"
  >;
}

function renderUseAuth(initialOptions: UseAuthOptions) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  let latest: UseAuthResult | null = null;
  let currentOptions = initialOptions;

  function Probe(props: { options: UseAuthOptions }) {
    latest = useAuth(props.options);
    return null;
  }

  const render = () => {
    act(() => {
      root.render(<Probe options={currentOptions} />);
    });
  };

  render();

  return {
    latest: () => {
      if (!latest) {
        throw new Error("hook did not produce a result");
      }
      return latest;
    },
    rerender: (nextOptions: UseAuthOptions) => {
      currentOptions = nextOptions;
      render();
    },
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

function buildSession(partial: Partial<AuthSession> = {}): AuthSession {
  return {
    accessToken: "at-1",
    accessExpiresAt: "2099-01-01T00:01:00.000Z",
    refreshToken: "rt-1",
    refreshExpiresAt: "2099-02-01T00:00:00.000Z",
    user: {
      id: "user-1",
      email: "alice@example.com",
      display_name: "Alice",
    },
    ...partial,
  };
}

function createStore(): AuthStore {
  return createAuthStore();
}

describe("useAuth", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  });

  it("hydrates auth store from stored session on mount", () => {
    const auth = createMockAuth();
    const session = buildSession();
    auth.getStoredSession.mockReturnValue(session);

    const store = createStore();
    const harness = renderUseAuth({
      auth: authOptions(auth),
      store,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
    });

    const state = harness.latest();
    expect(state.status).toBe("authenticated");
    expect(state.isAuthenticated).toBe(true);
    expect(state.user).toEqual(session.user);
    expect(state.accessToken).toBe("at-1");

    harness.unmount();
  });

  it("login delegates to startGitHubOAuth with callback redirect URI", async () => {
    const auth = createMockAuth();
    auth.getStoredSession.mockReturnValue(null);
    auth.startGitHubOAuth.mockResolvedValue({
      flowId: "flow-1",
      authorizationUrl: "https://github.com/login/oauth/authorize",
      expiresAt: "2099-01-01T00:00:00.000Z",
      state: "state-1",
    });

    const store = createStore();
    const harness = renderUseAuth({
      auth: authOptions(auth),
      store,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
    });

    await act(async () => {
      await harness.latest().login();
    });

    expect(auth.startGitHubOAuth).toHaveBeenCalledWith({
      redirectUri: "https://app.scriptum.dev/auth-callback",
    });

    harness.unmount();
  });

  it("logout calls auth.logout and clears the store", async () => {
    const auth = createMockAuth();
    auth.getStoredSession.mockReturnValue(buildSession());
    auth.logout.mockResolvedValue(undefined);

    const store = createStore();
    const harness = renderUseAuth({
      auth: authOptions(auth),
      store,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
    });

    await act(async () => {
      await harness.latest().logout();
    });

    expect(auth.logout).toHaveBeenCalledTimes(1);
    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().accessToken).toBeNull();
    expect(store.getState().user).toBeNull();

    harness.unmount();
  });

  it("refreshes access token before expiry and updates store", async () => {
    vi.useFakeTimers();
    const now = Date.parse("2026-02-07T00:00:00.000Z");
    vi.setSystemTime(now);

    const auth = createMockAuth();
    auth.getStoredSession.mockReturnValue(
      buildSession({
        accessToken: "at-old",
        accessExpiresAt: "2026-02-07T00:01:00.000Z",
      }),
    );
    auth.refreshAccessToken.mockResolvedValue(
      buildSession({
        accessToken: "at-new",
        refreshToken: "rt-new",
        accessExpiresAt: "2026-02-07T00:10:00.000Z",
      }),
    );

    const store = createStore();
    const harness = renderUseAuth({
      auth: authOptions(auth),
      store,
      refreshBufferMs: 30_000,
      location: { assign: vi.fn(), origin: "https://app.scriptum.dev" },
    });

    await act(async () => {
      vi.advanceTimersByTime(30_001);
      await Promise.resolve();
    });

    expect(auth.refreshAccessToken).toHaveBeenCalledTimes(1);
    expect(store.getState().accessToken).toBe("at-new");
    expect(store.getState().refreshToken).toBe("rt-new");
    expect(store.getState().status).toBe("authenticated");

    harness.unmount();
  });

  it("redirects to login path when token refresh fails", async () => {
    vi.useFakeTimers();
    const now = Date.parse("2026-02-07T00:00:00.000Z");
    vi.setSystemTime(now);

    const auth = createMockAuth();
    auth.getStoredSession.mockReturnValue(
      buildSession({
        accessExpiresAt: "2026-02-07T00:00:10.000Z",
      }),
    );
    auth.refreshAccessToken.mockResolvedValue(null);

    const assign = vi.fn();
    const store = createStore();
    const harness = renderUseAuth({
      auth: authOptions(auth),
      store,
      refreshBufferMs: 5_000,
      location: { assign, origin: "https://app.scriptum.dev" },
      loginPath: "/",
    });

    await act(async () => {
      vi.advanceTimersByTime(5_001);
      await Promise.resolve();
    });

    expect(auth.refreshAccessToken).toHaveBeenCalledTimes(1);
    expect(assign).toHaveBeenCalledWith("https://app.scriptum.dev/");
    expect(store.getState().status).toBe("unauthenticated");
    expect(store.getState().error).toContain("Session expired");

    harness.unmount();
  });
});
