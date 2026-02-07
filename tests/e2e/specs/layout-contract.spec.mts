import { expect, test, type Page } from "@playwright/test";
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
    };
  }
}

const smokeFixtures = loadSmokeFixtures();

interface LayoutContract {
  fixtureName: string;
  route: string;
  viewport: {
    width: number;
    height: number;
    devicePixelRatio: number;
  };
  rootTokens: Record<string, string>;
  elements: Record<
    string,
    {
      selector: string;
      bounds: {
        x: number;
        y: number;
        width: number;
        height: number;
      };
      typography: {
        fontSize: string;
        fontWeight: string;
        lineHeight: string;
        letterSpacing: string;
      };
      spacing: {
        marginTop: string;
        marginBottom: string;
        paddingTop: string;
        paddingBottom: string;
      };
      tokens: Record<string, string>;
    }
  >;
}

const TOKEN_NAMES = [
  "--tw-ring-color",
  "--tw-ring-offset-width",
  "--tw-ring-offset-color",
  "--tw-bg-opacity",
  "--tw-text-opacity",
  "--tw-border-opacity",
  "--tw-shadow",
  "--tw-shadow-color",
];

const CRITICAL_ELEMENTS: Array<{ key: string; selector: string }> = [
  { key: "html", selector: "html" },
  { key: "body", selector: "body" },
  { key: "appRoot", selector: "#root" },
  { key: "main", selector: "main" },
  { key: "primaryHeading", selector: "h1" },
  { key: "appShell", selector: '[data-testid="app-shell"]' },
  { key: "sidebar", selector: '[data-testid="app-sidebar"]' },
  { key: "editorArea", selector: '[data-testid="app-editor-area"]' },
  { key: "editorHost", selector: '[data-testid="editor-host"]' },
  { key: "tabBar", selector: '[data-testid="tab-bar"]' },
  { key: "statusBar", selector: '[data-testid="status-bar"]' },
  { key: "avatarStack", selector: '[data-testid="avatar-stack"]' },
  { key: "presenceStack", selector: '[data-testid="presence-stack"]' },
  { key: "overlapIndicator", selector: '[data-testid="overlap-indicator"]' },
  { key: "overlapSeverity", selector: '[data-testid="overlap-severity"]' },
  { key: "smokeProbe", selector: '[data-testid="smoke-fixture-probe"]' },
];

test.describe("layout contracts @smoke", () => {
  for (const fixture of smokeFixtures) {
    test(`${fixture.name} layout contract @smoke`, async ({ page }) => {
      await page.goto(fixture.route);
      await page.waitForLoadState("domcontentloaded");
      await expect(page.locator("body")).toBeVisible();

      await applyFixtureState(page, fixture.state);
      if (fixture.state) {
        await expect(page.getByTestId("smoke-fixture-probe")).toBeVisible();
      }

      const contract = await extractLayoutContract(page, fixture.name);
      expect(JSON.stringify(contract, null, 2)).toMatchSnapshot(
        `${fixture.name}.layout-contract.json`,
      );
    });
  }
});

async function applyFixtureState(
  page: Page,
  state?: SmokeFixtureState,
): Promise<void> {
  if (state) {
    await expect
      .poll(() => {
        return page.evaluate(() => Boolean(window.__SCRIPTUM_TEST__));
      })
      .toBe(true);
  }

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
    document.body.appendChild(probe);
  }, state ?? null);
}

async function extractLayoutContract(
  page: Page,
  fixtureName: string,
): Promise<LayoutContract> {
  return page.evaluate(
    ({ elementSpecs, fixture, tokenNames }) => {
      const round = (value: number): number => Math.round(value * 100) / 100;
      const readTokens = (
        style: CSSStyleDeclaration,
      ): Record<string, string> => {
        const tokens: Record<string, string> = {};
        for (const name of tokenNames) {
          const raw = style.getPropertyValue(name).trim();
          if (raw.length > 0) {
            tokens[name] = raw;
          }
        }
        return tokens;
      };

      const rootStyle = getComputedStyle(document.documentElement);
      const elements: LayoutContract["elements"] = {};
      for (const spec of elementSpecs) {
        const element = document.querySelector<HTMLElement>(spec.selector);
        if (!element) {
          continue;
        }
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        elements[spec.key] = {
          selector: spec.selector,
          bounds: {
            x: round(rect.x),
            y: round(rect.y),
            width: round(rect.width),
            height: round(rect.height),
          },
          typography: {
            fontSize: style.fontSize,
            fontWeight: style.fontWeight,
            lineHeight: style.lineHeight,
            letterSpacing: style.letterSpacing,
          },
          spacing: {
            marginTop: style.marginTop,
            marginBottom: style.marginBottom,
            paddingTop: style.paddingTop,
            paddingBottom: style.paddingBottom,
          },
          tokens: readTokens(style),
        };
      }

      return {
        fixtureName: fixture,
        route: window.location.pathname,
        viewport: {
          width: window.innerWidth,
          height: window.innerHeight,
          devicePixelRatio: window.devicePixelRatio,
        },
        rootTokens: readTokens(rootStyle),
        elements,
      };
    },
    {
      elementSpecs: CRITICAL_ELEMENTS,
      fixture: fixtureName,
      tokenNames: TOKEN_NAMES,
    },
  );
}
