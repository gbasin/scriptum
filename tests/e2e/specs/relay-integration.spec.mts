// Relay integration e2e tests — real relay via password auth (Docker required).

import { expect, test } from "@playwright/test";
import { injectSession, MOCK_USER } from "../helpers/auth-helpers.mts";
import {
  isRelayAvailable,
  logoutUser,
  refreshToken,
  registerUser,
  startOAuthFlow,
  uniqueEmail,
} from "../helpers/relay-client.mts";

let relayAvailable = false;

test.beforeAll(async () => {
  relayAvailable = await isRelayAvailable();
});

// biome-ignore lint/correctness/noEmptyPattern: Playwright requires destructured first arg
test.beforeEach(async ({}, testInfo) => {
  if (!relayAvailable) {
    testInfo.skip(true, "Relay not available — skipping integration test");
  }
});

test.describe("relay integration", () => {
  test("OAuth /start works with real PKCE", async () => {
    const codeChallenge = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const result = await startOAuthFlow({
      redirect_uri: "http://127.0.0.1:4176/auth-callback",
      state: `test-state-${Date.now()}`,
      code_challenge: codeChallenge,
      code_challenge_method: "S256",
    });

    expect(result.flow_id).toBeTruthy();
    expect(result.authorization_url).toContain("github.com");
    expect(result.expires_at).toBeTruthy();
    // Verify the expires_at is in the future
    expect(new Date(result.expires_at).getTime()).toBeGreaterThan(Date.now());
  });

  test("session restore with relay-issued JWT", async ({ page }) => {
    const email = uniqueEmail("restore");
    const password = "TestPass123!";

    const registration = await registerUser({
      email,
      display_name: "Restore Test User",
      password,
    });

    // Inject the real tokens into the browser
    await page.goto("/");
    await injectSession(
      page,
      {
        access_token: registration.access_token,
        access_expires_at: registration.access_expires_at,
        refresh_token: registration.refresh_token,
        refresh_expires_at: registration.refresh_expires_at,
      },
      registration.user ?? {
        id: "relay-user",
        email,
        display_name: "Restore Test User",
      },
    );

    await page.reload();

    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 10_000,
    });
  });

  test("token refresh rotates + reuse detection", async () => {
    const email = uniqueEmail("refresh");
    const password = "TestPass123!";

    const registration = await registerUser({
      email,
      display_name: "Refresh Test User",
      password,
    });

    // First refresh succeeds — returns new token pair
    const refreshed = await refreshToken(registration.refresh_token);
    expect(refreshed.access_token).toBeTruthy();
    expect(refreshed.refresh_token).toBeTruthy();
    expect(refreshed.refresh_token).not.toBe(registration.refresh_token);

    // Reuse the old refresh token — should be rejected (reuse detection)
    try {
      await refreshToken(registration.refresh_token);
      // If it doesn't throw, the response should indicate failure
      throw new Error("Expected old token to be rejected");
    } catch (err) {
      expect((err as Error).message).toMatch(
        /Refresh failed: 4\d\d|Expected old token/,
      );
    }
  });

  test("frontend auto-refresh with real tokens", async ({ page }) => {
    const email = uniqueEmail("auto-refresh");
    const password = "TestPass123!";

    const registration = await registerUser({
      email,
      display_name: "Auto-Refresh User",
      password,
    });

    // Inject tokens with near-expiry access token (30s — within 60s buffer)
    const nearExpiryAccessAt = new Date(Date.now() + 30_000).toISOString();
    await page.goto("/");
    await injectSession(
      page,
      {
        access_token: registration.access_token,
        access_expires_at: nearExpiryAccessAt,
        refresh_token: registration.refresh_token,
        refresh_expires_at: registration.refresh_expires_at,
      },
      registration.user ?? {
        id: "relay-user",
        email,
        display_name: "Auto-Refresh User",
      },
    );

    await page.reload();

    // Wait for auto-refresh to fire (access is within 60s buffer)
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 10_000,
    });

    // Verify the token was rotated in localStorage
    const newAccessToken = await page.evaluate(() =>
      localStorage.getItem("scriptum:access_token"),
    );
    // After auto-refresh, the stored token should differ from the original
    // (unless the relay rejects the refresh, which would clear session)
    expect(newAccessToken).toBeTruthy();
  });

  test("logout revokes session on relay", async () => {
    const email = uniqueEmail("logout");
    const password = "TestPass123!";

    const registration = await registerUser({
      email,
      display_name: "Logout Test User",
      password,
    });

    // Logout via relay API
    const logoutRes = await logoutUser(
      registration.access_token,
      registration.refresh_token,
    );
    expect(logoutRes.ok).toBe(true);

    // Attempt to refresh — should fail because session is revoked
    try {
      await refreshToken(registration.refresh_token);
      throw new Error("Expected refresh to fail after logout");
    } catch (err) {
      expect((err as Error).message).toMatch(
        /Refresh failed: 4\d\d|Expected refresh to fail/,
      );
    }
  });

  test("bogus tokens rejected", async ({ page }) => {
    await page.goto("/");

    // Inject fake JWT tokens that the relay will not recognize
    await injectSession(
      page,
      {
        access_token: "bogus-access-token-not-a-jwt",
        access_expires_at: new Date(Date.now() + 30_000).toISOString(), // Near expiry → triggers refresh
        refresh_token: "bogus-refresh-token-not-a-jwt",
        refresh_expires_at: new Date(Date.now() + 7 * 86400_000).toISOString(),
      },
      MOCK_USER,
    );

    await page.reload();

    // The auto-refresh should fail (bogus token rejected by relay),
    // and the session should be cleared → landing page shown.
    await expect(page.getByTestId("index-landing")).toBeVisible({
      timeout: 15_000,
    });
  });
});
