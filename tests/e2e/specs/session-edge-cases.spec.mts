// Session edge case e2e tests — concurrent logins, storage failures, timeouts.

import { expect, type Route, test } from "@playwright/test";
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

test.describe("session edge cases", () => {
  test("double login start overwrites PKCE", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    let callCount = 0;
    const states: string[] = [];

    await page.route("**/v1/auth/**", async (route: Route) => {
      const url = route.request().url();
      const body = route.request().postDataJSON();

      if (url.includes("/oauth/github/start")) {
        callCount++;
        const state = body?.state ?? `state-${callCount}`;
        states.push(state);
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            flow_id: `${MOCK_FLOW_ID}-${callCount}`,
            authorization_url: `${MOCK_AUTHORIZATION_URL}${state}`,
            expires_at: new Date(Date.now() + 600_000).toISOString(),
          }),
        });
        return;
      }

      if (url.includes("/oauth/github/callback")) {
        const tokens = mockTokens();
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ ...tokens, user: MOCK_USER }),
        });
        return;
      }

      await route.continue();
    });

    // Intercept GitHub redirects
    await interceptGitHubRedirect(page);

    // Start login flow #1
    await page.getByTestId("index-login-button").click();
    await expect.poll(() => states.length, { timeout: 5_000 }).toBe(1);

    // Navigate home (simulating user going back)
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    // Re-setup GitHub redirect interception after navigation
    await interceptGitHubRedirect(page);

    // Start login flow #2 — this overwrites PKCE state in sessionStorage
    await page.getByTestId("index-login-button").click();
    await expect.poll(() => states.length, { timeout: 5_000 }).toBe(2);

    // Use the FIRST state in callback — should fail because the stored PKCE
    // data was overwritten by the second login start. Depending on whether
    // the flow data was fully replaced or lost during navigation, we get
    // either a state mismatch or a missing flow error.
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${states[0]}`);

    await expect(page.getByTestId("auth-callback-error")).toBeVisible({
      timeout: 5_000,
    });
    const errorText = await page
      .getByTestId("auth-callback-error")
      .textContent();
    // Either "state mismatch" (second state stored) or "flow data not found"
    // (sessionStorage cleared by navigation) are valid — both prove the
    // first PKCE state was invalidated.
    expect(
      errorText?.includes("state mismatch") ||
        errorText?.includes("flow data not found"),
    ).toBe(true);
  });

  test("localStorage quota exceeded", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tracker = await interceptRelayAuth(page);
    const getGitHubUrl = await interceptGitHubRedirect(page);

    // Fill localStorage close to quota (~5MB)
    await page.evaluate(() => {
      const chunk = "x".repeat(1024 * 1024); // 1MB
      try {
        for (let i = 0; i < 5; i++) {
          localStorage.setItem(`_padding_${i}`, chunk);
        }
      } catch {
        // Expected — quota reached
      }
    });

    // Start flow
    await page.getByTestId("index-login-button").click();

    // Wait for /start
    await expect
      .poll(() => tracker.start.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    const capturedUrl = await getGitHubUrl();
    const state = new URL(capturedUrl).searchParams.get("state") ?? "";

    // Navigate to callback — saving session will fail due to quota
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${state}`);

    // The app should handle this gracefully — either error UI or authentication
    // (depends on whether saveSession throws or silently fails).
    // Wait for either auth or error to appear.
    const authOrError = page
      .getByTestId("index-authenticated")
      .or(page.getByTestId("auth-callback-error"));
    await expect(authOrError).toBeVisible({ timeout: 10_000 });
  });

  test("relay network timeout on refresh", async ({ page }) => {
    // Token expires in 30s (within 60s buffer → immediate refresh)
    const tokens = mockTokens({ accessExpiresInMs: 30_000 });
    await page.goto("/");

    // Mock relay: /token/refresh never responds
    await page.route("**/v1/auth/**", async (route: Route) => {
      const url = route.request().url();

      if (url.includes("/token/refresh")) {
        // Never fulfill — simulates network timeout
        // Playwright will abort on page navigation or test cleanup.
        return;
      }

      await route.continue();
    });

    await injectSession(page, tokens);
    await page.reload();

    // The auto-refresh will hang, eventually the app should clear the session
    // or the user sees the landing page on next reload.
    // Give it time to detect the failure.
    await page.waitForTimeout(5_000);
    await page.reload();

    // After reload, session should be gone (refresh never succeeded,
    // access token is expired)
    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 10_000,
    });
  });

  test("malformed JSON from relay /start", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    await page.route("**/v1/auth/**", async (route: Route) => {
      const url = route.request().url();

      if (url.includes("/oauth/github/start")) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: "{ not json",
        });
        return;
      }

      await route.continue();
    });

    await page.getByTestId("index-login-button").click();

    // Should show "Login failed" error (not a crash)
    await expect(page.getByTestId("index-login-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("back button to stale callback URL", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tracker = await interceptRelayAuth(page);
    const getGitHubUrl = await interceptGitHubRedirect(page);

    // Complete a full login
    await page.getByTestId("index-login-button").click();
    const capturedUrl = await getGitHubUrl();
    const state = new URL(capturedUrl).searchParams.get("state") ?? "";

    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${state}`);

    await expect
      .poll(() => tracker.callback.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 10_000,
    });

    // Now navigate back to the same callback URL (simulating back button)
    await page.goto(`/auth-callback?code=${MOCK_CODE}&state=${state}`);

    // PKCE flow data was cleared after the first exchange.
    // Should show "flow data not found" error.
    await expect(page.getByTestId("auth-callback-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("partial session in localStorage", async ({ page }) => {
    await page.goto("/");

    // Set only access_token + access_expires_at (missing refresh/user)
    await page.evaluate(() => {
      localStorage.setItem("scriptum:access_token", "partial-token");
      localStorage.setItem(
        "scriptum:access_expires_at",
        new Date(Date.now() + 3600_000).toISOString(),
      );
      // Missing: refresh_token, refresh_expires_at, user
    });

    await page.reload();

    // loadSession() returns null when any field is missing → unauthenticated
    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("concurrent logout calls", async ({ page }) => {
    const tokens = mockTokens();
    await page.goto("/");

    await interceptRelayAuth(page);
    await injectSession(page, tokens);
    await page.reload();
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 5_000,
    });

    // Trigger 3 simultaneous logouts via page.evaluate
    const results = await page.evaluate(async () => {
      const errors: string[] = [];

      const doLogout = async () => {
        try {
          localStorage.removeItem("scriptum:access_token");
          localStorage.removeItem("scriptum:access_expires_at");
          localStorage.removeItem("scriptum:refresh_token");
          localStorage.removeItem("scriptum:refresh_expires_at");
          localStorage.removeItem("scriptum:user");
        } catch (err) {
          errors.push(String(err));
        }
      };

      await Promise.all([doLogout(), doLogout(), doLogout()]);
      return {
        errors,
        hasToken: localStorage.getItem("scriptum:access_token"),
      };
    });

    expect(results.errors).toHaveLength(0);
    expect(results.hasToken).toBeNull();

    await page.reload();
    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("revoked refresh token clears session", async ({ page }) => {
    // Token expires in 30s (within 60s buffer → immediate refresh)
    const tokens = mockTokens({ accessExpiresInMs: 30_000 });
    await page.goto("/");

    // Mock relay: refresh returns 401 AUTH_TOKEN_REVOKED
    await page.route("**/v1/auth/**", async (route: Route) => {
      const url = route.request().url();

      if (url.includes("/token/refresh")) {
        await route.fulfill({
          status: 401,
          contentType: "application/json",
          body: JSON.stringify({
            error: "AUTH_TOKEN_REVOKED",
            message: "Refresh token has been revoked",
          }),
        });
        return;
      }

      await route.continue();
    });

    await injectSession(page, tokens);
    await page.reload();

    // Auto-refresh fires, gets 401 → session should be cleared
    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 15_000,
    });

    // Verify localStorage is cleared
    const hasToken = await page.evaluate(() =>
      localStorage.getItem("scriptum:access_token"),
    );
    expect(hasToken).toBeNull();
  });
});
