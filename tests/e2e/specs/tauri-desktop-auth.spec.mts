// Tauri desktop auth e2e tests — browser-level flow with mocked Tauri APIs.

import { expect, test } from "@playwright/test";
import {
  interceptRelayAuth,
  MOCK_CODE,
  MOCK_USER,
  mockTokens,
} from "../helpers/auth-helpers.mts";
import {
  emitDeepLink,
  getKeychainTokens,
  getLastBrowserUrl,
  injectTauriMock,
  overrideTauriCommand,
} from "../helpers/tauri-mock.mts";

test.describe("Tauri desktop auth", () => {
  test.beforeEach(async ({ page }) => {
    // Inject Tauri mocks before every test — must happen before page.goto
    await injectTauriMock(page);
  });

  test("login detects Tauri, opens system browser", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    // Set up relay mock for /start
    await interceptRelayAuth(page);

    await page.getByTestId("index-login-button").click();

    // Wait for the system browser URL to be set
    await expect
      .poll(async () => getLastBrowserUrl(page), { timeout: 5_000 })
      .toBeTruthy();

    const browserUrl = await getLastBrowserUrl(page);
    expect(browserUrl).toContain("github.com");

    // Page should NOT have navigated away — still on the SPA
    await expect(page.getByTestId("index-landing")).toBeVisible();
  });

  test("deep link callback completes auth", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tokens = mockTokens();
    const tracker = await interceptRelayAuth(page, {
      callbackBody: { ...tokens, user: MOCK_USER },
    });

    await page.getByTestId("index-login-button").click();

    // Wait for the system browser URL to be set (flow started)
    await expect
      .poll(async () => getLastBrowserUrl(page), { timeout: 5_000 })
      .toBeTruthy();

    const browserUrl = await getLastBrowserUrl(page);
    const state = new URL(browserUrl as string).searchParams.get("state") ?? "";

    // Simulate deep link callback from OS
    await emitDeepLink(
      page,
      `scriptum://auth/callback?code=${MOCK_CODE}&state=${state}`,
    );

    // Wait for the callback exchange
    await expect
      .poll(() => tracker.callback.length, { timeout: 5_000 })
      .toBeGreaterThan(0);

    // Should be authenticated
    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 10_000,
    });
  });

  test("deep link with error payload", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    await interceptRelayAuth(page);

    await page.getByTestId("index-login-button").click();

    // Wait for the system browser to open
    await expect
      .poll(async () => getLastBrowserUrl(page), { timeout: 5_000 })
      .toBeTruthy();

    // Simulate deep link with error
    await emitDeepLink(
      page,
      "scriptum://auth/callback?error=access_denied&error_description=User+denied+access",
    );

    // Should show login error
    await expect(page.getByTestId("index-login-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("keychain persistence after login", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    const tokens = mockTokens();
    await interceptRelayAuth(page, {
      callbackBody: { ...tokens, user: MOCK_USER },
    });

    await page.getByTestId("index-login-button").click();

    // Wait for system browser
    await expect
      .poll(async () => getLastBrowserUrl(page), { timeout: 5_000 })
      .toBeTruthy();

    const browserUrl = await getLastBrowserUrl(page);
    const state = new URL(browserUrl as string).searchParams.get("state") ?? "";

    // Complete the flow via deep link
    await emitDeepLink(
      page,
      `scriptum://auth/callback?code=${MOCK_CODE}&state=${state}`,
    );

    await expect(page.getByTestId("index-authenticated")).toBeVisible({
      timeout: 10_000,
    });

    // Verify keychain has tokens
    const keychainTokens = await getKeychainTokens(page);
    expect(keychainTokens).toBeTruthy();
  });

  test("Tauri command failure fallback", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    await interceptRelayAuth(page);

    // Override auth_redirect_uri to throw an error
    await overrideTauriCommand(
      page,
      "auth_redirect_uri",
      'throw new Error("Tauri IPC unavailable");',
    );

    await page.getByTestId("index-login-button").click();

    // Should show a login error (caught by useAuth)
    await expect(page.getByTestId("index-login-error")).toBeVisible({
      timeout: 5_000,
    });
  });

  test("no deep link emitted — login stays pending", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("index-landing")).toBeVisible();

    await interceptRelayAuth(page);

    await page.getByTestId("index-login-button").click();

    // Wait for system browser to open
    await expect
      .poll(async () => getLastBrowserUrl(page), { timeout: 5_000 })
      .toBeTruthy();

    // Wait a short period — no deep link will come
    await page.waitForTimeout(2_000);

    // The landing page should still be visible (login in progress, not completed).
    // The SPA doesn't navigate away in Tauri mode.
    await expect(page.getByTestId("index-landing")).toBeVisible();

    // Should NOT show authenticated state
    await expect(page.getByTestId("index-authenticated")).not.toBeVisible();
  });
});
