import { expect, type Page, test } from "@playwright/test";
import { loadSmokeFixtures, type SmokeFixture } from "./smoke-fixtures.mts";

interface ScriptumTestApi {
  setDocContent(markdown: string): void;
  setCursor(pos: { line: number; ch: number }): void;
  setSyncState(state: "synced" | "offline" | "reconnecting" | "error"): void;
}

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: ScriptumTestApi;
  }
}

const RICH_FIXTURE_NAME = "editor-live-preview-rich";
const richFixture = loadRichFixture();

test.describe("mermaid live preview @smoke", () => {
  test("renders mermaid widget SVG with expected flowchart nodes @smoke", async ({
    page,
  }) => {
    await page.goto(richFixture.route);
    await expect(page.getByTestId("document-title")).toBeVisible();
    await expect(
      page.locator('[data-testid="editor-host"] .cm-editor'),
    ).toBeVisible();

    await installDeterministicMermaidRenderer(page);
    await applyFixtureMarkdown(page, richFixture);

    const editorHost = page.getByTestId("editor-host");
    await expect(
      editorHost.locator(".cm-livePreview-mermaidBlock"),
    ).toBeVisible({
      timeout: 20_000,
    });
    const mermaidSvg = editorHost.locator(".cm-livePreview-mermaidBlock svg");
    await expect(mermaidSvg).toBeVisible({ timeout: 20_000 });
    await expect(
      editorHost.locator(".cm-livePreview-mermaidFallback"),
    ).toHaveCount(0);

    const nodeLabels = (await mermaidSvg.locator("text").allTextContents())
      .map((label) => label.trim())
      .filter((label) => label.length > 0);
    expect(nodeLabels).toEqual(
      expect.arrayContaining(["Draft", "Review", "Publish"]),
    );

    const editorText =
      (await editorHost.locator(".cm-content").textContent()) ?? "";
    expect(editorText).not.toContain("```mermaid");

    await expect(page).toHaveScreenshot(
      "editor-live-preview-rich-mermaid.png",
      {
        fullPage: true,
      },
    );
  });
});

function loadRichFixture(): SmokeFixture {
  const fixture = loadSmokeFixtures().find(
    (candidate) => candidate.name === RICH_FIXTURE_NAME,
  );
  if (!fixture) {
    throw new Error(`missing smoke fixture: ${RICH_FIXTURE_NAME}`);
  }
  if (!fixture.state?.docContent) {
    throw new Error(
      `fixture ${RICH_FIXTURE_NAME} must provide state.docContent`,
    );
  }
  return fixture;
}

async function applyFixtureMarkdown(
  page: Page,
  fixture: SmokeFixture,
): Promise<void> {
  const docContent = fixture.state?.docContent;
  if (!docContent) {
    throw new Error(`fixture ${fixture.name} has no state.docContent`);
  }

  await page.evaluate(
    ({ cursor, markdown, syncState }) => {
      const api = window.__SCRIPTUM_TEST__;
      if (!api) {
        throw new Error("__SCRIPTUM_TEST__ was not installed in fixture mode");
      }

      api.setDocContent(markdown);
      api.setCursor(cursor ?? { line: 0, ch: 0 });
      api.setSyncState(syncState ?? "synced");
    },
    {
      cursor: fixture.state?.cursor,
      markdown: docContent,
      syncState: fixture.state?.syncState,
    },
  );
}

async function installDeterministicMermaidRenderer(page: Page): Promise<void> {
  await page.evaluate(() => {
    const parseLabels = (source: string): string[] => {
      const labels = Array.from(source.matchAll(/\[([^\]]+)\]/g))
        .map((match) => match[1]?.trim() ?? "")
        .filter((label) => label.length > 0);
      return Array.from(new Set(labels));
    };

    (
      window as unknown as {
        mermaid?: { render: (id: string, source: string) => { svg: string } };
      }
    ).mermaid = {
      render: (_id: string, source: string) => {
        const labels = parseLabels(source);
        const height = Math.max(80, 36 + labels.length * 24);
        const textNodes = labels
          .map(
            (label, index) =>
              `<text x="24" y="${30 + index * 24}" font-size="14">${label}</text>`,
          )
          .join("");
        return {
          svg: `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="${height}" viewBox="0 0 320 ${height}">${textNodes}</svg>`,
        };
      },
    };
  });
}
