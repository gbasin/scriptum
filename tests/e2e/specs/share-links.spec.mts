import { expect, test } from "@playwright/test";

interface ScriptumTestApi {
  reset(): void;
  setDocContent(markdown: string): void;
  setSyncState(state: "synced" | "offline" | "reconnecting" | "error"): void;
  setShareLinksEnabled(enabled: boolean): void;
}

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: ScriptumTestApi;
  }
}

const ROUTE = "/workspace/ws-share/document/doc-share";

test.describe("share links e2e @smoke", () => {
  test("creates a share link and redeems it with configured controls @smoke", async ({
    page,
  }) => {
    await page.goto(ROUTE);
    await expect(page.getByTestId("document-title")).toBeVisible();
    await expect(
      page.locator('[data-testid="editor-host"] .cm-editor'),
    ).toBeVisible();

    await page.evaluate(() => {
      const api = window.__SCRIPTUM_TEST__;
      if (!api) {
        throw new Error("__SCRIPTUM_TEST__ was not installed in fixture mode");
      }

      api.reset();
      api.setDocContent("# Share link test");
      api.setSyncState("synced");
      api.setShareLinksEnabled(true);
    });

    await expect(page.getByTestId("share-link-open")).toBeVisible();
    await page.getByTestId("share-link-open").click();

    const dialog = page.getByTestId("share-link-dialog");
    await expect(dialog).toBeVisible();

    const targetOptions = (
      await page
        .getByTestId("share-link-target")
        .locator("option")
        .allTextContents()
    ).map((value) => value.trim());
    expect(targetOptions).toEqual(["Workspace", "Document"]);

    const permissionOptions = (
      await page
        .getByTestId("share-link-permission")
        .locator("option")
        .allTextContents()
    ).map((value) => value.trim());
    expect(permissionOptions).toEqual(["Viewer", "Editor"]);

    await page.getByTestId("share-link-target").selectOption("document");
    await page.getByTestId("share-link-permission").selectOption("edit");
    await page.getByTestId("share-link-expiration").selectOption("24h");
    await page.getByTestId("share-link-max-uses").fill("3");
    await page.getByTestId("share-link-generate").click();

    await expect(page.getByTestId("share-link-summary")).toContainText(
      "editor",
    );
    await expect(page.getByTestId("share-link-summary")).toContainText(
      "document",
    );

    const generatedUrl = await page.getByTestId("share-link-url").inputValue();
    expect(generatedUrl).toContain("/share/");

    await page.goto(generatedUrl);
    await expect(page.getByTestId("share-redeem-title")).toBeVisible();
    await expect(page.getByTestId("share-redeem-target")).toContainText(
      "document/doc-share",
    );
    await expect(page.getByTestId("share-redeem-permission")).toContainText(
      "editor",
    );
    await expect(page.getByTestId("share-redeem-max-uses")).toContainText("3");
    await expect(page.getByTestId("share-redeem-use-count")).toContainText("0");
    await expect(page.getByTestId("share-redeem-expiration")).not.toContainText(
      "Never",
    );

    await page.getByTestId("share-redeem-submit").click();
    await expect(page.getByTestId("share-redeem-success")).toContainText(
      "redeemed",
    );
    await expect(page.getByTestId("share-redeem-use-count")).toContainText("1");
  });
});
