import { expect, type Page, test } from "@playwright/test";
import {
  SMOKE_FIXTURES,
  type SmokeFixture,
} from "../../src/test/smoke-fixtures";

async function applyFixture(page: Page, fixture: SmokeFixture) {
  await page.goto(fixture.route);
  await page.waitForFunction(() => Boolean(window.__SCRIPTUM_TEST__));

  await page.evaluate((value) => {
    const api = window.__SCRIPTUM_TEST__;
    if (!api) {
      throw new Error("__SCRIPTUM_TEST__ API is unavailable");
    }

    api.reset();
    if (value.loadFixture) {
      api.loadFixture(value.loadFixture);
    }
    if (value.docContent !== undefined) {
      api.setDocContent(value.docContent);
    }
    if (value.syncState !== undefined) {
      api.setSyncState(value.syncState);
    }
    if (value.gitStatus !== undefined) {
      api.setGitStatus(value.gitStatus);
    }
    if (value.remotePeers) {
      for (const peer of value.remotePeers) {
        api.spawnRemotePeer(peer);
      }
    }
  }, fixture);
}

for (const fixture of SMOKE_FIXTURES) {
  test(`smoke fixture: ${fixture.id}`, async ({ page }) => {
    await applyFixture(page, fixture);

    await expect(page.getByTestId("app-sidebar")).toBeVisible();
    await expect(page.getByTestId("editor-surface")).toBeVisible();
    await expect(page.getByTestId("sync-state")).toContainText(
      fixture.syncState ?? "synced",
    );

    if (fixture.docContent) {
      await expect(page.getByTestId("editor-content")).toContainText(
        fixture.docContent,
      );
    }

    if (fixture.remotePeers && fixture.remotePeers.length > 0) {
      await expect(page.getByTestId("presence-stack")).toContainText(
        fixture.remotePeers[0].name,
      );
    }

    await expect(page.getByTestId("app-editor-area")).toMatchAriaSnapshot();
    await expect(page).toHaveScreenshot(`${fixture.id}.png`, {
      animations: "disabled",
      fullPage: true,
    });
  });
}
