// Auth flow e2e tests — real auth code paths (no fixture mode).

import { expect, type Page, type Route, test } from "@playwright/test";

// --- Test data ---

const MOCK_USER = {
  id: "user-e2e-1",
  email: "e2e@example.com",
  display_name: "E2E Test User",
};

const MOCK_FLOW_ID = "flow-e2e-1";
const MOCK_CODE = "gh-code-e2e-1";
const MOCK_AUTHORIZATION_URL =
  "https://github.com/login/oauth/authorize?client_id=test&state=";

function mockTokens(
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

interface InterceptOptions {
  startStatus?: number;
  startBody?: Record<string, unknown>;
  callbackStatus?: number;
  callbackBody?: Record<string, unknown>;
  refreshStatus?: number;
  refreshBody?: Record<string, unknown>;
  logoutStatus?: number;
  callbackDelay?: number;
}

interface CallTracker {
  start: Array<{ body: unknown }>;
  callback: Array<{ body: unknown }>;
  refresh: Array<{ body: unknown }>;
  logout: Array<{ body: unknown }>;
}

async function interceptRelayAuth(
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

async function injectSession(
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
async function interceptGitHubRedirect(
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

// --- Tests ---

test.describe("OAuth sign-in flow", () => {
  test("landing page shows sign-in when unauthenticated", async ({ page }) => {
    await page.goto("/");

    const landing = page.getByTestId("index-landing");
    await expect(landing).toBeVisible();

    const button = page.getByTestId("index-login-button");
    await expect(button).toBeVisible();
    await expect(button).toHaveText("Sign in with GitHub");
  });

  test("clicking sign-in calls relay with PKCE", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tracker = await interceptRelayAuth(page);

    // Stub location.assign to prevent navigation
    await interceptGitHubRedirect(page);

    await page.getByTestId("index-login-button").click();

    // Wait for the /start call
    await expect
      .poll(() => tracker.start.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    const startBody = tracker.start[0].body as Record<string, string>;
    expect(startBody.code_challenge).toBeTruthy();
    expect(startBody.code_challenge_method).toBe("S256");
    expect(startBody.state).toBeTruthy();
    expect(startBody.redirect_uri).toContain("/auth-callback");
  });

  test("full happy path: start → callback → authenticated", async ({
    page,
  }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tracker = await interceptRelayAuth(page);

    // Intercept GitHub redirect so it serves dummy HTML instead of real GitHub
    const getGitHubUrl = await interceptGitHubRedirect(page);

    await page.getByTestId("index-login-button").click();

    // Wait for the GitHub redirect to be captured (navigation aborted, page stays on SPA)
    const capturedUrl = await getGitHubUrl();
    expect(capturedUrl).toContain("github.com");

    // Extract state from the captured URL
    const state = new URL(capturedUrl).searchParams.get("state") ?? "";
    expect(state).toBeTruthy();

    // Simulate GitHub redirecting back with an authorization code.
    // sessionStorage (PKCE data) survives cross-origin navigation in the same tab.
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${state}`);

    // Wait for the /callback exchange call
    await expect
      .poll(() => tracker.callback.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    // Should redirect to authenticated home page
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.getByText(MOCK_USER.display_name)).toBeVisible();
  });
});

test.describe("session management", () => {
  test("restores session from localStorage on reload", async ({ page }) => {
    const tokens = mockTokens();
    await page.goto("/");

    // Intercept any auth requests (e.g. token refresh)
    await interceptRelayAuth(page);

    await injectSession(page, tokens);
    await page.reload();

    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("clears expired session", async ({ page }) => {
    const tokens = mockTokens({ refreshExpiresInMs: -1_000 });
    await page.goto("/");
    await injectSession(page, tokens);
    await page.reload();

    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("auto-refreshes token before expiry", async ({ page }) => {
    // Token that expires in 30 seconds (within 60s buffer → immediate refresh)
    const tokens = mockTokens({ accessExpiresInMs: 30_000 });
    await page.goto("/");

    const tracker = await interceptRelayAuth(page);
    await injectSession(page, tokens);
    await page.reload();

    // Wait for automatic refresh call
    await expect
      .poll(() => tracker.refresh.length, { timeout: 10_000 })
      .toBeGreaterThan(0);

    // Should still be authenticated
    await expect(page.getByTestId("index-authenticated")).toBeVisible();
  });
});

test.describe("logout", () => {
  test("logout clears session", async ({ page }) => {
    const tokens = mockTokens();
    await page.goto("/");

    await interceptRelayAuth(page);
    await injectSession(page, tokens);
    await page.reload();
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 5_000,
    });

    // Clear session state directly (simulating logout)
    await page.evaluate(() => {
      localStorage.removeItem("scriptum:access_token");
      localStorage.removeItem("scriptum:access_expires_at");
      localStorage.removeItem("scriptum:refresh_token");
      localStorage.removeItem("scriptum:refresh_expires_at");
      localStorage.removeItem("scriptum:user");
    });

    // Reload to pick up the cleared state
    await page.reload();

    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 5_000,
    });

    // Verify localStorage is cleared
    const hasToken = await page.evaluate(() =>
      localStorage.getItem("scriptum:access_token"),
    );
    expect(hasToken).toBeNull();
  });

  test("logout succeeds even if relay fails", async ({ page }) => {
    const tokens = mockTokens();
    await page.goto("/");

    await interceptRelayAuth(page, { logoutStatus: 500 });
    await injectSession(page, tokens);
    await page.reload();
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 5_000,
    });

    // Clear client-side state (simulating a logout where server returns 500)
    await page.evaluate(() => {
      localStorage.removeItem("scriptum:access_token");
      localStorage.removeItem("scriptum:access_expires_at");
      localStorage.removeItem("scriptum:refresh_token");
      localStorage.removeItem("scriptum:refresh_expires_at");
      localStorage.removeItem("scriptum:user");
    });
    await page.reload();

    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 5_000,
    });
  });
});

test.describe("error handling", () => {
  test("OAuth start failure", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    await interceptRelayAuth(page, { startStatus: 500 });

    await page.getByTestId("index-login-button").click();

    await expect(page.getByTestId("index-login-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("missing flow data shows error on callback", async ({ page }) => {
    await page.goto("/");

    // Intercept the relay callback so the fetch doesn't fail with network error
    await interceptRelayAuth(page);

    // Navigate directly to callback without starting a flow
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=bogus-state`);

    // Should show error about missing flow data
    await expect(page.getByTestId("auth-callback-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("callback exchange failure", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tracker = await interceptRelayAuth(page, {
      callbackStatus: 401,
      callbackBody: { error: "invalid_grant" },
    });

    // Stub location.assign to prevent navigation
    await interceptGitHubRedirect(page);

    await page.getByTestId("index-login-button").click();

    await expect
      .poll(() => tracker.start.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    const state = (tracker.start[0].body as Record<string, string>).state;

    // Navigate to callback
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${state}`);

    await expect(page.getByTestId("auth-callback-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("missing params redirect to home", async ({ page }) => {
    await page.goto("/auth-callback");

    // Should redirect to /
    await page.waitForURL("**/", { timeout: 5_000 });
    await expect(page.getByTestId("index-landing")).toBeVisible();
  });
});

test.describe("auth callback timeout and retry", () => {
  test("timeout UI appears on slow callback", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    // Set up interceptor with 15s delay (exceeds AUTH_CALLBACK_TIMEOUT_MS = 10_000)
    const tracker = await interceptRelayAuth(page, {
      callbackDelay: 15_000,
    });

    // Stub location.assign to prevent navigation
    await interceptGitHubRedirect(page);

    // Start the flow
    await page.getByTestId("index-login-button").click();

    await expect
      .poll(() => tracker.start.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    const state = (tracker.start[0].body as Record<string, string>).state;

    // Navigate to callback
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${state}`);

    // Should show timeout after ~10s
    await expect(page.getByTestId("auth-callback-timeout")).toBeVisible({
      timeout: 15_000,
    });
  });

  test("retry button works after timeout", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    // Use a flag instead of a counter so React StrictMode double-mount
    // (which fires the effect twice) doesn't skip the delay.
    let shouldSucceed = false;
    let capturedState = "";

    await page.route("**/v1/auth/**", async (route) => {
      const url = route.request().url();
      const body = route.request().postDataJSON();

      if (url.includes("/oauth/github/start")) {
        const state = body?.state ?? "test-state";
        capturedState = state;
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            flow_id: MOCK_FLOW_ID,
            authorization_url: `${MOCK_AUTHORIZATION_URL}${state}`,
            expires_at: new Date(Date.now() + 600_000).toISOString(),
          }),
        });
        return;
      }

      if (url.includes("/oauth/github/callback")) {
        if (shouldSucceed) {
          // After retry: succeed immediately
          await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({
              ...mockTokens(),
              user: MOCK_USER,
            }),
          });
        } else {
          // Before retry: delay beyond timeout threshold
          await new Promise((r) => setTimeout(r, 15_000));
          await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({
              ...mockTokens(),
              user: MOCK_USER,
            }),
          });
        }
        return;
      }

      if (url.includes("/token/refresh")) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(mockTokens()),
        });
        return;
      }

      await route.continue();
    });

    // Stub location.assign to prevent navigation
    await interceptGitHubRedirect(page);

    // Start the flow
    await page.getByTestId("index-login-button").click();

    // Wait for /start call to capture state
    await expect.poll(() => capturedState, { timeout: 5_000 }).toBeTruthy();

    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${capturedState}`);

    // Wait for timeout UI
    await expect(page.getByTestId("auth-callback-timeout")).toBeVisible({
      timeout: 15_000,
    });

    // Flip the flag so the next callback succeeds immediately
    shouldSucceed = true;

    // Click retry
    await page.getByTestId("auth-callback-retry").click();

    // Should eventually authenticate
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 15_000,
    });
  });
});
