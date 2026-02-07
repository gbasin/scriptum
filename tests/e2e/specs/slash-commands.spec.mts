import { expect, type Page, test } from "@playwright/test";
import {
  createSlashCommandInsertion,
  getSlashCommand,
} from "../../../packages/editor/src/index";

interface ScriptumTestApi {
  reset(): void;
  setCursor(pos: { line: number; ch: number }): void;
  setDocContent(markdown: string): void;
  setSyncState(state: "synced" | "offline" | "reconnecting" | "error"): void;
}

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: ScriptumTestApi;
  }
}

const ROUTE = "/workspace/ws-slash/document/doc-slash";
const EXPECTED_LABELS = ["/table", "/code", "/image", "/callout"];
const tableInsertion = createSlashCommandInsertion(
  getSlashCommand("table"),
  0,
  1,
);
const EXPECTED_TABLE_DOC = tableInsertion.changes.insert;
const EXPECTED_TABLE_CURSOR = cursorFromOffset(
  EXPECTED_TABLE_DOC,
  tableInsertion.selection.anchor,
);
const EXPECTED_TABLE_LINES = EXPECTED_TABLE_DOC.split("\n");
const EXPECTED_CURSOR_LABEL = `Ln ${EXPECTED_TABLE_CURSOR.line + 1}, Col ${EXPECTED_TABLE_CURSOR.ch + 1}`;

test.describe("slash commands e2e @smoke", () => {
  test("opens slash palette, inserts table template, and positions cursor @smoke", async ({
    page,
  }) => {
    await setupSlashFixture(page);

    const editorContent = page.locator(
      '[data-testid="editor-host"] .cm-content',
    );
    await editorContent.click();
    await page.keyboard.type("/");

    const dropdown = page.locator(".cm-tooltip-autocomplete");
    await expect(dropdown).toBeVisible();

    const labels = (
      await dropdown.locator(".cm-completionLabel").allTextContents()
    ).map((label) => label.trim());
    expect(labels).toEqual(EXPECTED_LABELS);

    await expect(dropdown).toMatchAriaSnapshot();

    await dropdown
      .locator(".cm-completionLabel")
      .filter({ hasText: "/table" })
      .first()
      .click();
    await expect(dropdown).toHaveCount(0);

    const lineTexts = (
      await editorContent.locator(".cm-line").allTextContents()
    ).map(normalizeRenderedLineText);
    expect(lineTexts.slice(0, EXPECTED_TABLE_LINES.length)).toEqual(
      EXPECTED_TABLE_LINES,
    );

    await expect(page.getByTestId("status-bar")).toContainText(
      EXPECTED_CURSOR_LABEL,
    );
  });
});

async function setupSlashFixture(page: Page): Promise<void> {
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
    api.setDocContent("");
    api.setCursor({ line: 0, ch: 0 });
    api.setSyncState("synced");
  });
}

function cursorFromOffset(
  text: string,
  offset: number,
): { ch: number; line: number } {
  const clampedOffset = Math.max(0, Math.min(offset, text.length));
  const prefix = text.slice(0, clampedOffset);
  const lines = prefix.split("\n");
  const line = Math.max(0, lines.length - 1);
  const ch = lines[lines.length - 1]?.length ?? 0;
  return { ch, line };
}

function normalizeRenderedLineText(lineText: string): string {
  return lineText.replaceAll("\u00a0", " ").replaceAll("\u200b", "");
}
