import { expect, type Page, test } from "@playwright/test";

interface RemotePeerFixture {
  name: string;
  type: "human" | "agent";
  cursor: { line: number; ch: number };
  section?: string;
}

interface PresenceScenario {
  expectedSeverity: "info" | "warning";
  name: string;
  peers: RemotePeerFixture[];
}

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: {
      getState(): { remotePeers: RemotePeerFixture[] };
      reset(): void;
      setCursor(pos: { line: number; ch: number }): void;
      setDocContent(markdown: string): void;
      setSyncState(
        state: "synced" | "offline" | "reconnecting" | "error",
      ): void;
      spawnRemotePeer(peer: RemotePeerFixture): void;
    };
  }
}

const ROUTE = "/workspace/ws-smoke/document/doc-smoke";

const scenarios: PresenceScenario[] = [
  {
    expectedSeverity: "info",
    name: "presence-overlap-info",
    peers: [
      {
        name: "Alex Human",
        type: "human",
        cursor: { line: 2, ch: 2 },
        section: "section-a",
      },
      {
        name: "Scriptum Bot",
        type: "agent",
        cursor: { line: 4, ch: 1 },
        section: "section-b",
      },
    ],
  },
  {
    expectedSeverity: "warning",
    name: "presence-overlap-warning",
    peers: [
      {
        name: "Alex Human",
        type: "human",
        cursor: { line: 2, ch: 2 },
        section: "shared-auth",
      },
      {
        name: "Scriptum Bot",
        type: "agent",
        cursor: { line: 2, ch: 12 },
        section: "shared-auth",
      },
    ],
  },
];

test.describe("presence + overlap overlays @smoke", () => {
  for (const scenario of scenarios) {
    test(`${scenario.name} @smoke`, async ({ page }) => {
      await page.goto(ROUTE);
      await applyScenario(page, scenario);

      const stateSnapshot = await page.evaluate(() => {
        return window.__SCRIPTUM_TEST__?.getState() ?? null;
      });
      expect(stateSnapshot).not.toBeNull();
      expect(stateSnapshot?.remotePeers.length ?? 0).toBe(
        scenario.peers.length,
      );

      await expect(
        page.locator(".cm-remote-cursor-label").first(),
      ).toBeVisible();
      const cursorLabels = (
        await page.locator(".cm-remote-cursor-label").allTextContents()
      ).map((value) => value.trim());
      expect(cursorLabels.length).toBeGreaterThan(0);
      expect(
        cursorLabels.some((label) =>
          scenario.peers.some((peer) => peer.name === label),
        ),
      ).toBe(true);

      const presenceItems = (
        await page
          .locator('[data-testid="presence-stack"] li')
          .allTextContents()
      ).map((value) => value.trim());
      for (const peer of scenario.peers) {
        expect(presenceItems).toContain(`${peer.name} (${peer.type})`);
      }

      const severity = await page
        .getByTestId("overlap-severity")
        .getAttribute("data-severity");
      expect(severity).toBe(scenario.expectedSeverity);

      for (const peer of scenario.peers) {
        await expect(
          page.getByTestId(`attribution-badge-${badgeSuffix(peer.name)}`),
        ).toContainText(peer.type === "agent" ? "AGENT" : "HUMAN");
      }

      await expect(page).toHaveScreenshot(`${scenario.name}-overlays.png`, {
        fullPage: true,
      });
      await expect(page.locator("body")).toMatchAriaSnapshot();
    });
  }
});

function badgeSuffix(name: string): string {
  const normalized = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || "peer";
}

async function applyScenario(
  page: Page,
  scenario: PresenceScenario,
): Promise<void> {
  await expect(
    page.locator('[data-testid="editor-host"] .cm-editor'),
  ).toBeVisible();

  await page.evaluate(({ peers }) => {
    const api = window.__SCRIPTUM_TEST__;
    if (!api) {
      throw new Error("__SCRIPTUM_TEST__ was not installed");
    }

    api.reset();
    api.setDocContent(
      "# Overlap Harness\n\n## Section A\nAlpha\n\n## Section B\nBeta\n",
    );
    api.setSyncState("synced");
    api.setCursor({ line: 0, ch: 0 });
    for (const peer of peers) {
      api.spawnRemotePeer(peer);
    }
  }, scenario);

  await expect(page.getByTestId("editor-host")).toBeVisible();
  await expect(page.getByTestId("presence-stack")).toBeVisible();
  await expect(page.getByTestId("overlap-indicator")).toBeVisible();
}
