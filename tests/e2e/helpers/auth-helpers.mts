// Shared auth test helpers — mock data, relay interceptor, session injection.

import type { Page, Route } from "@playwright/test";

// --- Test data ---

export const MOCK_USER = {
  id: "user-e2e-1",
  email: "e2e@example.com",
  display_name: "E2E Test User",
};

export const MOCK_FLOW_ID = "flow-e2e-1";
export const MOCK_CODE = "gh-code-e2e-1";
export const MOCK_AUTHORIZATION_URL =
  "https://github.com/login/oauth/authorize?client_id=test&state=";

export function mockTokens(
  overrides: { accessExpiresInMs?: number; refreshExpiresInMs?: number } = {},
) {
  const now = Date.now();
  return {
    access_token: `access-${Date.now()}`,
    access_expires_at: new Date(
      now + (overrides.accessExpiresInMs ?? 3600_000),
    ).toISOString(),
    refresh_token: `refresh-${Date.now()}`,
    refresh_expires_at: new Date(
      now + (overrides.refreshExpiresInMs ?? 7 * 86400_000),
    ).toISOString(),
  };
}

// --- Relay endpoint interceptor ---

export interface InterceptOptions {
  startStatus?: number;
  startBody?: Record<string, unknown>;
  callbackStatus?: number;
  callbackBody?: Record<string, unknown>;
  refreshStatus?: number;
  refreshBody?: Record<string, unknown>;
  logoutStatus?: number;
  callbackDelay?: number;
}

export interface CallTracker {
  start: Array<{ body: unknown }>;
  callback: Array<{ body: unknown }>;
  refresh: Array<{ body: unknown }>;
  logout: Array<{ body: unknown }>;
}

export async function interceptRelayAuth(
  page: Page,
  options: InterceptOptions = {},
): Promise<CallTracker> {
  const tracker: CallTracker = {
    start: [],
    callback: [],
    refresh: [],
    logout: [],
  };

  const relayPattern = "**/v1/auth/**";

  await page.route(relayPattern, async (route: Route) => {
    const url = route.request().url();
    const body = route.request().postDataJSON();

    if (url.includes("/oauth/github/start")) {
      tracker.start.push({ body });
      const state = body?.state ?? "test-state";
      await route.fulfill({
        status: options.startStatus ?? 200,
        contentType: "application/json",
        body: JSON.stringify(
          options.startBody ?? {
            flow_id: MOCK_FLOW_ID,
            authorization_url: `${MOCK_AUTHORIZATION_URL}${state}`,
            expires_at: new Date(Date.now() + 600_000).toISOString(),
          },
        ),
      });
      return;
    }

    if (url.includes("/oauth/github/callback")) {
      tracker.callback.push({ body });

      if (options.callbackDelay) {
        await new Promise((r) => setTimeout(r, options.callbackDelay));
      }

      const tokens = mockTokens();
      await route.fulfill({
        status: options.callbackStatus ?? 200,
        contentType: "application/json",
        body: JSON.stringify(
          options.callbackBody ?? {
            ...tokens,
            user: MOCK_USER,
          },
        ),
      });
      return;
    }

    if (url.includes("/token/refresh")) {
      tracker.refresh.push({ body });
      const tokens = mockTokens();
      await route.fulfill({
        status: options.refreshStatus ?? 200,
        contentType: "application/json",
        body: JSON.stringify(options.refreshBody ?? tokens),
      });
      return;
    }

    if (url.includes("/logout")) {
      tracker.logout.push({ body });
      await route.fulfill({
        status: options.logoutStatus ?? 200,
        contentType: "application/json",
        body: JSON.stringify({ success: true }),
      });
      return;
    }

    await route.continue();
  });

  return tracker;
}

// --- Session injection helper ---

export async function injectSession(
  page: Page,
  tokens: ReturnType<typeof mockTokens>,
  user = MOCK_USER,
) {
  await page.evaluate(
    ({ tokens, user }) => {
      localStorage.setItem("scriptum:access_token", tokens.access_token);
      localStorage.setItem(
        "scriptum:access_expires_at",
        tokens.access_expires_at,
      );
      localStorage.setItem("scriptum:refresh_token", tokens.refresh_token);
      localStorage.setItem(
        "scriptum:refresh_expires_at",
        tokens.refresh_expires_at,
      );
      localStorage.setItem("scriptum:user", JSON.stringify(user));
    },
    { tokens, user },
  );
}

/**
 * Intercepts the GitHub OAuth redirect. The `startGitHubOAuth` function
 * calls `location.assign(authorization_url)` which navigates the whole
 * page away. We intercept the navigation at the network level, fulfilling
 * GitHub requests with a minimal page, then wait for the page to settle
 * before returning.
 *
 * Returns a function that waits for and returns the captured authorization URL.
 */
export async function interceptGitHubRedirect(
  page: Page,
): Promise<() => Promise<string>> {
  let firstUrl = "";
  let resolve: ((url: string) => void) | null = null;
  const promise = new Promise<string>((r) => {
    resolve = r;
  });

  await page.route("https://github.com/**", async (route) => {
    const url = route.request().url();
    if (!firstUrl) {
      firstUrl = url;
      resolve?.(url);
    }
    await route.fulfill({
      status: 200,
      contentType: "text/html",
      body: "<html><body>Redirecting...</body></html>",
    });
  });

  return async () => {
    const url = await Promise.race([
      promise,
      new Promise<string>((_, reject) =>
        setTimeout(
          () => reject(new Error("GitHub redirect not captured")),
          5_000,
        ),
      ),
    ]);
    // Wait for the navigation to fully complete before the test continues.
    // This prevents "interrupted by another navigation" errors.
    await page.waitForLoadState("networkidle");
    return url;
  };
}
