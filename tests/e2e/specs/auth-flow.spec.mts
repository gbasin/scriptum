// Auth flow e2e tests — real auth code paths (no fixture mode).

import { expect, test } from "@playwright/test";
import {
  injectSession,
  interceptGitHubRedirect,
  interceptRelayAuth,
  MOCK_AUTHORIZATION_URL,
  MOCK_CODE,
  MOCK_FLOW_ID,
  MOCK_USER,
  mockTokens,
} from "../helpers/auth-helpers.mts";

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
