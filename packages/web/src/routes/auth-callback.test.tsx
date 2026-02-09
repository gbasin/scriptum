// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuthClient } from "../auth/client";
import { AUTH_CALLBACK_TIMEOUT_MS, AuthCallbackRoute } from "./auth-callback";

const mockNavigate = vi.fn();

const authStoreState = {
  error: null as string | null,
  handleCallback: vi.fn(
    async (_client: AuthClient, _code: string, _state: string) => undefined,
  ),
  status: "unknown" as "unknown" | "authenticated" | "unauthenticated",
};

vi.mock("react-router-dom", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

vi.mock("../store/auth", () => ({
  useAuthStore: (selector: (state: typeof authStoreState) => unknown) =>
    selector(authStoreState),
}));

vi.mock("../test/setup", () => ({
  isFixtureModeEnabled: () => false,
}));

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function renderRoute(path: string): { container: HTMLDivElement; root: Root } {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/auth-callback" element={<AuthCallbackRoute />} />
        </Routes>
      </MemoryRouter>,
    );
  });

  return { container, root };
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  authStoreState.error = null;
  authStoreState.status = "unknown";
  authStoreState.handleCallback.mockReset();
  authStoreState.handleCallback.mockResolvedValue(undefined);
  mockNavigate.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  document.body.innerHTML = "";
});

describe("AuthCallbackRoute", () => {
  it("passes OAuth callback params to auth store handler", () => {
    const { root } = renderRoute(
      "/auth-callback?code=gh-code&state=csrf-token",
    );

    expect(authStoreState.handleCallback).toHaveBeenCalledTimes(1);
    const [client, code, state] = authStoreState.handleCallback.mock.calls[0]!;
    expect(client).toBeInstanceOf(AuthClient);
    expect(code).toBe("gh-code");
    expect(state).toBe("csrf-token");

    act(() => {
      root.unmount();
    });
  });

  it("redirects home when callback params are missing", () => {
    const { root } = renderRoute("/auth-callback");

    expect(authStoreState.handleCallback).not.toHaveBeenCalled();
    expect(mockNavigate).toHaveBeenCalledWith("/", { replace: true });

    act(() => {
      root.unmount();
    });
  });

  it("shows timeout state and retries callback exchange", () => {
    vi.useFakeTimers();
    authStoreState.handleCallback.mockImplementation(
      () => new Promise(() => undefined),
    );

    const { container, root } = renderRoute(
      "/auth-callback?code=slow-code&state=slow-state",
    );

    act(() => {
      vi.advanceTimersByTime(AUTH_CALLBACK_TIMEOUT_MS + 1);
    });

    expect(
      container.querySelector('[data-testid="auth-callback-timeout"]'),
    ).not.toBeNull();

    const retryButton = container.querySelector(
      '[data-testid="auth-callback-retry"]',
    ) as HTMLButtonElement | null;
    act(() => {
      retryButton?.click();
    });

    expect(authStoreState.handleCallback).toHaveBeenCalledTimes(2);

    act(() => {
      root.unmount();
    });
  });

  it("navigates home when auth status becomes authenticated", () => {
    authStoreState.status = "authenticated";
    const { root } = renderRoute("/auth-callback?code=ok&state=ok");

    expect(mockNavigate).toHaveBeenCalledWith("/", { replace: true });

    act(() => {
      root.unmount();
    });
  });
});
