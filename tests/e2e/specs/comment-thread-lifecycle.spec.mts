import { expect, type Page, test } from "@playwright/test";

interface ScriptumTestApi {
  reset(): void;
  setCursor(pos: { line: number; ch: number }): void;
  setDocContent(markdown: string): void;
  setSyncState(
    state: "synced" | "offline" | "reconnecting" | "error",
  ): void;
}

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: ScriptumTestApi;
  }
}

const ROUTE = "/workspace/ws-comments/document/doc-comments";
const FIXED_TIMESTAMP_ISO = "2026-01-01T00:00:00.000Z";
const INITIAL_MESSAGE = "Thread start from E2E";
const REPLY_MESSAGE = "Reply from E2E";

test.describe("comment thread lifecycle e2e @smoke", () => {
  test("create, reply, resolve, and reopen thread with gutter + aria coverage @smoke", async ({
    page,
  }) => {
    await setupCommentFixture(page);

    await selectCommentAnchorWord(page);
    await expect(page.getByTestId("comment-margin-button")).toBeVisible();
    await page.getByTestId("comment-margin-button").click();

    const popover = page.getByTestId("comment-popover");
    await expect(popover).toBeVisible();
    await expect(popover).toMatchAriaSnapshot();

    await page.getByTestId("comment-input").fill(INITIAL_MESSAGE);
    await page.getByTestId("comment-submit").click();
    await expect(popover).toHaveCount(0);

    const threadList = page.getByTestId("comment-threads");
    await expect(threadList).toContainText(INITIAL_MESSAGE);
    await expect(
      page.locator('[data-testid="editor-host"] .cm-commentGutterMarker-open'),
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="editor-host"] .cm-commentHighlight-open'),
    ).toBeVisible();

    await page.getByTestId("comment-margin-button").click();
    await expect(popover).toBeVisible();
    await expect(page.getByTestId("comment-thread-replies")).toBeVisible();

    await page.getByTestId("comment-input").fill(REPLY_MESSAGE);
    await page.getByTestId("comment-submit").click();
    await expect(page.getByTestId("comment-thread-replies")).toContainText(
      REPLY_MESSAGE,
    );
    await expect(threadList).toContainText(REPLY_MESSAGE);

    await page.getByTestId("comment-thread-resolve").click();
    await expect(page.getByTestId("comment-thread-collapsed")).toBeVisible();
    await expect(page.getByTestId("comment-thread-resolved-note")).toBeVisible();
    await expect(
      page.locator('[data-testid="editor-host"] .cm-commentGutterMarker-resolved'),
    ).toBeVisible();
    await expect(threadList).toContainText("Resolved");

    await page.getByTestId("comment-thread-reopen").click();
    await expect(page.getByTestId("comment-thread-replies")).toBeVisible();
    await expect(page.getByTestId("comment-input")).toBeVisible();
    await expect(threadList).toContainText("Open");
    await expect(
      page.locator('[data-testid="editor-host"] .cm-commentGutterMarker-open'),
    ).toBeVisible();

    await expect(threadList).toMatchAriaSnapshot();
  });
});

async function setupCommentFixture(page: Page): Promise<void> {
  await page.addInitScript((fixedTimestampIso) => {
    const fixedNow = new Date(fixedTimestampIso).valueOf();
    Date.now = () => fixedNow;
  }, FIXED_TIMESTAMP_ISO);

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
    api.setDocContent(
      [
        "# Comment Lifecycle",
        "",
        "Select sentence for thread coverage.",
        "Second line remains untouched.",
      ].join("\n"),
    );
    api.setCursor({ line: 2, ch: 7 });
    api.setSyncState("synced");
  });
}

async function selectCommentAnchorWord(page: Page): Promise<void> {
  const targetLine = page
    .locator('[data-testid="editor-host"] .cm-line')
    .filter({ hasText: "Select sentence for thread coverage." })
    .first();
  await expect(targetLine).toBeVisible();
  await targetLine.click({
    position: { x: 12, y: 8 },
  });

  await page.keyboard.down("Shift");
  for (let index = 0; index < 8; index += 1) {
    await page.keyboard.press("ArrowRight");
  }
  await page.keyboard.up("Shift");
}
