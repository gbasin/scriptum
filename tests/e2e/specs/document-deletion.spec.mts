import { expect, type Page, test } from "@playwright/test";

const ROUTE = "/workspace/ws-delete/document/doc-delete";

test.describe("document deletion e2e @smoke", () => {
  test("deletes active document via context menu after confirmation @smoke", async ({
    page,
  }) => {
    await page.goto(ROUTE);
    await expect(page.getByTestId("document-title")).toBeVisible();

    await page.getByTestId("create-workspace-button").click();
    await expect(page.getByTestId("workspace-dropdown")).toHaveValue(/ws-/);

    const firstPath = await createUntitledDocument(page);
    const secondPath = await createUntitledDocument(page);

    const targetNode = page.getByTestId(`tree-node-${secondPath}`);
    await expect(targetNode).toHaveAttribute("data-active", "true");
    await targetNode.locator("button").click({ button: "right" });

    await expect(page.getByTestId("context-menu")).toBeVisible();
    await page.getByTestId("context-action-delete").click();

    const dialog = page.getByTestId("delete-document-dialog");
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(secondPath);
    await expect(dialog).toMatchAriaSnapshot();

    await page.getByTestId("delete-document-confirm").click();

    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId(`tree-node-${secondPath}`)).toHaveCount(0);
    await expect(page.getByTestId(`tree-node-${firstPath}`)).toHaveAttribute(
      "data-active",
      "true",
    );
    await expect(
      page.locator('[data-testid="editor-host"] .cm-editor'),
    ).toBeVisible();
  });
});

async function createUntitledDocument(page: Page): Promise<string> {
  await page.locator("body").click();
  await page.keyboard.press("ControlOrMeta+n");

  const renameInput = page
    .locator('[data-testid^="tree-rename-input-"]')
    .first();
  await expect(renameInput).toBeVisible();

  const path = await renameInput.inputValue();
  await renameInput.press("Enter");
  await expect(renameInput).toHaveCount(0);
  await expect(page.getByTestId(`tree-node-${path}`)).toBeVisible();

  return path;
}
