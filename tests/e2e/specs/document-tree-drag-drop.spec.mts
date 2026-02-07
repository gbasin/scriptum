import { expect, type Page, test } from "@playwright/test";

const ROUTE = "/workspace/ws-drag/document/doc-drag";

test.describe("document tree drag-drop e2e @smoke", () => {
  test("reorders siblings and moves a document into a folder @smoke", async ({
    page,
  }) => {
    await page.goto(ROUTE);
    await expect(page.getByTestId("document-title")).toBeVisible();

    await ensureWorkspaceExists(page);
    await createDocumentViaShortcut(page, "alpha.md");
    await createDocumentViaShortcut(page, "beta.md");
    await createDocumentViaShortcut(page, "gamma.md");
    await createDocumentViaShortcut(page, "projects/notes.md");
    await ensureFolderExpanded(page, "projects");

    await expect.poll(() => readRootNodeLabels(page)).toEqual([
      "projects",
      "alpha.md",
      "beta.md",
      "gamma.md",
    ]);

    await expect(page.getByTestId("document-tree")).toHaveScreenshot(
      "document-tree-drag-before.png",
    );

    await page
      .locator('[data-testid="tree-node-gamma.md"] button')
      .dragTo(page.locator('[data-testid="tree-node-alpha.md"] button'));

    await expect.poll(() => readRootNodeLabels(page)).toEqual([
      "projects",
      "gamma.md",
      "alpha.md",
      "beta.md",
    ]);

    await page
      .locator('[data-testid="tree-node-beta.md"] button')
      .dragTo(page.locator('[data-testid="tree-node-projects"] button'));

    await ensureFolderExpanded(page, "projects");
    await expect(page.getByTestId("tree-node-projects/beta.md")).toBeVisible();
    await expect(page.locator('[data-testid="tree-node-beta.md"]')).toHaveCount(
      0,
    );

    await expect.poll(() => readRootNodeLabels(page)).toEqual([
      "projects",
      "gamma.md",
      "alpha.md",
    ]);

    await expect(page.getByTestId("document-tree")).toHaveScreenshot(
      "document-tree-drag-after.png",
    );
  });
});

async function ensureWorkspaceExists(page: Page): Promise<void> {
  const dropdown = page.getByTestId("workspace-dropdown");
  await expect(dropdown).toBeVisible();

  if ((await dropdown.inputValue()) === "") {
    await page.getByTestId("create-workspace-button").click();
    await expect(dropdown).not.toHaveValue("");
  }
}

async function triggerNewDocumentShortcut(page: Page): Promise<void> {
  await page.evaluate(() => {
    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        ctrlKey: true,
        key: "n",
      }),
    );
  });
}

async function createDocumentViaShortcut(
  page: Page,
  nextPath: string,
): Promise<void> {
  await triggerNewDocumentShortcut(page);
  const renameInput = page.locator('input[data-testid^="tree-rename-input-"]');
  await expect(renameInput).toBeVisible();
  await renameInput.fill(nextPath);
  await renameInput.press("Enter");

  const parent = parentPath(nextPath);
  if (parent.length > 0) {
    await ensureFolderExpanded(page, parent);
  }
  await expect(page.getByTestId(`tree-node-${nextPath}`)).toBeVisible();
}

async function ensureFolderExpanded(
  page: Page,
  folderPath: string,
): Promise<void> {
  const folderNode = page.getByTestId(`tree-node-${folderPath}`);
  await expect(folderNode).toBeVisible();
  const isExpanded = await folderNode.getAttribute("aria-expanded");
  if (isExpanded !== "true") {
    await folderNode.locator("button").click();
  }
}

async function readRootNodeLabels(page: Page): Promise<string[]> {
  return page
    .locator(
      '[data-testid="document-tree"] > ul[role="tree"] > li[role="treeitem"] > button',
    )
    .evaluateAll((buttons) =>
      buttons.map((button) => (button.getAttribute("aria-label") ?? "").trim()),
    );
}

function parentPath(path: string): string {
  const index = path.lastIndexOf("/");
  if (index < 0) {
    return "";
  }
  return path.slice(0, index);
}
