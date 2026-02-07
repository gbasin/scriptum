import { expect, type Page, test } from "@playwright/test";
import {
  loadSmokeFixtures,
  type SmokeFixtureState,
} from "./smoke-fixtures.mts";

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: {
      loadFixture(name: string): void;
      reset(): void;
      setDocContent(markdown: string): void;
      setCursor(pos: { line: number; ch: number }): void;
      spawnRemotePeer(peer: {
        name: string;
        type: "human" | "agent";
        cursor: { line: number; ch: number };
        section?: string;
      }): void;
      setGitStatus(status: {
        dirty: boolean;
        ahead: number;
        behind: number;
        lastCommit?: string;
      }): void;
      setSyncState(
        state: "synced" | "offline" | "reconnecting" | "error",
      ): void;
      setPendingSyncUpdates(count: number): void;
      setReconnectProgress(
        progress: { syncedUpdates: number; totalUpdates: number } | null,
      ): void;
      getState(): {
        fixtureName: string;
        docContent: string;
        cursor: { line: number; ch: number };
        remotePeers: Array<{
          name: string;
          type: "human" | "agent";
          cursor: { line: number; ch: number };
          section?: string;
        }>;
        syncState: "synced" | "offline" | "reconnecting" | "error";
        pendingSyncUpdates: number;
        reconnectProgress: {
          syncedUpdates: number;
          totalUpdates: number;
        } | null;
        gitStatus: {
          dirty: boolean;
          ahead: number;
          behind: number;
          lastCommit?: string;
        };
      };
    };
  }
}

const smokeFixtures = loadSmokeFixtures();
const LIVE_PREVIEW_FIXTURE_STYLE_ID = "__scriptum-live-preview-stabilize";

test.describe("ui smoke fixtures @smoke", () => {
  for (const fixture of smokeFixtures) {
    test(`${fixture.name} @smoke`, async ({ page }) => {
      await page.goto(fixture.route);
      await expect(
        page.getByText(fixture.expectations.heading).first(),
      ).toBeVisible();

      await applyFixtureState(page, fixture.state);
      if (fixture.state) {
        await expect(page.getByTestId("smoke-fixture-probe")).toBeVisible();
      }

      const stateSnapshot = await page.evaluate(() => {
        return window.__SCRIPTUM_TEST__?.getState() ?? null;
      });

      if (fixture.expectations.syncState) {
        expect(stateSnapshot?.syncState).toBe(fixture.expectations.syncState);
      }
      if (fixture.expectations.remotePeerCount !== undefined) {
        expect(stateSnapshot?.remotePeers.length ?? 0).toBe(
          fixture.expectations.remotePeerCount,
        );
      }
      if (fixture.state?.pendingSyncUpdates !== undefined) {
        expect(stateSnapshot?.pendingSyncUpdates).toBe(
          fixture.state.pendingSyncUpdates,
        );
      }
      if (fixture.state?.reconnectProgress !== undefined) {
        expect(stateSnapshot?.reconnectProgress).toEqual(
          fixture.state.reconnectProgress,
        );
      }
      if (fixture.state?.gitStatus?.lastCommit !== undefined) {
        expect(stateSnapshot?.gitStatus?.lastCommit).toBe(
          fixture.state.gitStatus.lastCommit,
        );
      }

      if (fixture.name === "editor-live-preview-rich") {
        await stabilizeLivePreviewFixture(page);
        await assertLivePreviewRawVsRich(page);
      }

      await expect(page).toHaveScreenshot(`${fixture.name}.png`, {
        fullPage: true,
      });
      await expect(page.locator("body")).toMatchAriaSnapshot();
    });
  }
});

async function applyFixtureState(
  page: Page,
  state?: SmokeFixtureState,
): Promise<void> {
  await page.evaluate((fixtureState) => {
    const probeId = "__scriptum-smoke-probe";
    document.getElementById(probeId)?.remove();

    if (!fixtureState) {
      return;
    }

    const api = window.__SCRIPTUM_TEST__;
    if (!api) {
      throw new Error("__SCRIPTUM_TEST__ was not installed in fixture mode");
    }

    api.reset();
    if (fixtureState.fixtureName) {
      api.loadFixture(fixtureState.fixtureName);
    }
    if (fixtureState.docContent !== undefined) {
      api.setDocContent(fixtureState.docContent);
    }
    if (fixtureState.cursor) {
      api.setCursor(fixtureState.cursor);
    }
    if (fixtureState.syncState) {
      api.setSyncState(fixtureState.syncState);
    }
    if (fixtureState.pendingSyncUpdates !== undefined) {
      api.setPendingSyncUpdates(fixtureState.pendingSyncUpdates);
    }
    if (fixtureState.reconnectProgress !== undefined) {
      api.setReconnectProgress(fixtureState.reconnectProgress);
    }
    if (fixtureState.gitStatus) {
      api.setGitStatus(fixtureState.gitStatus);
    }
    for (const peer of fixtureState.remotePeers ?? []) {
      api.spawnRemotePeer(peer);
    }

    const state = api.getState();
    const probe = document.createElement("section");
    probe.id = probeId;
    probe.setAttribute("aria-label", "Smoke fixture probe");
    probe.setAttribute("data-testid", "smoke-fixture-probe");
    probe.style.position = "fixed";
    probe.style.right = "16px";
    probe.style.bottom = "16px";
    probe.style.maxWidth = "360px";
    probe.style.padding = "10px";
    probe.style.border = "1px solid #111827";
    probe.style.background = "#f9fafb";
    probe.style.color = "#111827";
    probe.style.font = "12px/1.4 'SF Mono', Menlo, Monaco, monospace";
    probe.style.zIndex = "1000";
    const peers =
      state.remotePeers.length === 0
        ? "none"
        : state.remotePeers
            .map(
              (peer) =>
                `${peer.name}:${peer.type}@${peer.cursor.line}:${peer.cursor.ch}`,
            )
            .join(", ");
    probe.innerHTML = `
      <h2 style="margin: 0 0 6px 0; font-size: 12px;">Smoke Fixture Probe</h2>
      <p style="margin: 0;">fixture=${state.fixtureName}</p>
      <p style="margin: 0;">sync=${state.syncState}</p>
      <p style="margin: 0;">cursor=${state.cursor.line}:${state.cursor.ch}</p>
      <p style="margin: 0;">peers=${peers}</p>
      <p style="margin: 0;">pending=${state.pendingSyncUpdates}</p>
      <p style="margin: 0;">reconnect=${state.reconnectProgress ? `${state.reconnectProgress.syncedUpdates}/${state.reconnectProgress.totalUpdates}` : "none"}</p>
      <p style="margin: 0;">git=dirty:${state.gitStatus.dirty} ahead:${state.gitStatus.ahead} behind:${state.gitStatus.behind} last:${state.gitStatus.lastCommit ?? "none"}</p>
    `;
    document.body.appendChild(probe);
  }, state ?? null);
}

async function stabilizeLivePreviewFixture(page: Page): Promise<void> {
  await page.evaluate((styleId) => {
    document.getElementById(styleId)?.remove();
    const style = document.createElement("style");
    style.id = styleId;
    style.textContent = `
      [data-testid="editor-host"] {
        margin-inline: auto;
        max-width: 760px;
        width: 100%;
      }
      [data-testid="editor-host"] .cm-cursor,
      [data-testid="editor-host"] .cm-dropCursor {
        animation: none !important;
      }
      [data-testid="editor-host"] .cm-editor,
      [data-testid="editor-host"] .cm-scroller {
        scroll-behavior: auto !important;
      }
      [data-testid="editor-host"] .cm-content {
        font-family: "Scriptum Test Mono", "SF Mono", Menlo, Monaco, monospace !important;
        max-width: 720px;
        min-width: 0;
        white-space: pre-wrap;
      }
    `.trim();
    (document.head ?? document.documentElement).appendChild(style);
  }, LIVE_PREVIEW_FIXTURE_STYLE_ID);
}

async function assertLivePreviewRawVsRich(page: Page): Promise<void> {
  const probe = page.getByTestId("editor-host");
  await expect(probe.locator(".cm-editor")).toBeVisible({ timeout: 20_000 });
  await expect(probe.locator(".cm-content .cm-line").first()).toContainText(
    "# Active raw heading",
    { timeout: 20_000 },
  );

  const snapshot = await probe
    .locator(".cm-content .cm-line")
    .evaluateAll((lineNodes) => {
      const firstLine = lineNodes.at(0)?.textContent?.trim() ?? "";
      const secondLine = lineNodes.at(1)?.textContent?.trim() ?? "";

      return {
        firstLine,
        secondLine,
        editorText:
          document.querySelector('[data-testid="editor-host"] .cm-content')
            ?.textContent ?? "",
        hasCodeBlock: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-codeBlock',
          ),
        ),
        hasHeadingH2: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-heading-h2',
          ),
        ),
        hasLink: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-link',
          ),
        ),
        hasTable: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-table',
          ),
        ),
        hasTaskCheckbox: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-task-checkbox',
          ),
        ),
        hasInlineMathWidget: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-mathInline',
          ),
        ),
        hasDisplayMathWidget: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-mathBlock',
          ),
        ),
        hasInlineKatexClass: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-mathInline .katex',
          ),
        ),
        hasDisplayKatexClass: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-mathBlock .katex',
          ),
        ),
        hasCenteredDisplayEquation: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-mathBlock .katex-display',
          ),
        ),
        hasMathFallback: Boolean(
          document.querySelector(
            '[data-testid="editor-host"] .cm-livePreview-mathFallback',
          ),
        ),
      };
    });

  expect(snapshot.firstLine).toContain("# Active raw heading");
  expect(snapshot.firstLine.startsWith("# ")).toBe(true);
  expect(snapshot.secondLine).toContain("Inactive rendered heading");
  expect(snapshot.secondLine.startsWith("## ")).toBe(false);

  expect(snapshot.hasHeadingH2).toBe(true);
  expect(snapshot.hasTaskCheckbox).toBe(true);
  expect(snapshot.hasLink).toBe(true);
  expect(snapshot.hasTable).toBe(true);
  expect(snapshot.hasCodeBlock).toBe(true);
  expect(snapshot.hasInlineMathWidget).toBe(true);
  expect(snapshot.hasDisplayMathWidget).toBe(true);
  expect(snapshot.hasInlineKatexClass).toBe(true);
  expect(snapshot.hasDisplayKatexClass).toBe(true);
  expect(snapshot.hasCenteredDisplayEquation).toBe(true);
  expect(snapshot.hasMathFallback).toBe(false);
  expect(snapshot.editorText).not.toContain("$E=mc^2$");
  expect(snapshot.editorText).not.toContain("$$");
}
