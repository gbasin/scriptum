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

test.describe("ui smoke fixtures @smoke", () => {
  for (const fixture of smokeFixtures) {
    test(`${fixture.name} @smoke`, async ({ page }) => {
      await page.goto(fixture.route);
      await expect(page.getByText(fixture.expectations.heading)).toBeVisible();

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
      <p style="margin: 0;">git=dirty:${state.gitStatus.dirty} ahead:${state.gitStatus.ahead} behind:${state.gitStatus.behind}</p>
    `;
    document.body.appendChild(probe);
  }, state ?? null);
}
