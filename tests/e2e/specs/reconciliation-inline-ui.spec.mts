import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";
import {
  ReconciliationDetector,
  type ReconciliationInlineEntry,
} from "../../../packages/editor/src/index";

interface FixtureCursorPosition {
  line: number;
  ch: number;
}

interface FixtureRemotePeer {
  name: string;
  type: "human" | "agent";
  cursor: FixtureCursorPosition;
  section?: string;
}

interface ReconciliationEventFixture {
  authorId: string;
  timestampMs: number;
  changedChars: number;
  sectionLength: number;
}

interface ReconciliationVersionFixture {
  authorId: string;
  authorName: string;
  content: string;
}

interface ReconciliationFixtureMetadata {
  sectionId: string;
  sectionHeading: string;
  currentContent: string;
  windowMs: number;
  thresholdRatio: number;
  events: ReconciliationEventFixture[];
  versionA: ReconciliationVersionFixture;
  versionB: ReconciliationVersionFixture;
  expectedTrigger: {
    distinctAuthorCount: number;
    changeRatio: number;
  };
}

interface ReconciliationFixtureDocument {
  name: string;
  route: string;
  state: {
    fixtureName?: string;
    docContent: string;
    cursor?: FixtureCursorPosition;
    remotePeers?: FixtureRemotePeer[];
    syncState?: "synced" | "offline" | "reconnecting" | "error";
    pendingSyncUpdates?: number;
    reconnectProgress?: {
      syncedUpdates: number;
      totalUpdates: number;
    } | null;
    gitStatus?: {
      dirty: boolean;
      ahead: number;
      behind: number;
      lastCommit?: string;
    };
  };
  reconciliation: ReconciliationFixtureMetadata;
}

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: {
      loadFixture(name: string): void;
      reset(): void;
      setDocContent(markdown: string): void;
      setCursor(pos: FixtureCursorPosition): void;
      spawnRemotePeer(peer: FixtureRemotePeer): void;
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
      setReconciliationEntries(entries: ReconciliationInlineEntry[]): void;
    };
  }
}

const fixture = loadFixture();

test.describe("reconciliation inline UI @smoke", () => {
  test("renders conflicting versions and resolves with Keep A @smoke", async ({
    page,
  }) => {
    const entry = buildReconciliationEntry(fixture);
    await applyFixtureWithReconciliation(page, fixture, entry);

    const widget = page.locator(`[data-reconciliation-id="${entry.id}"]`);
    await expect(widget).toBeVisible();
    await expect(widget).toContainText(
      `Version A by ${fixture.reconciliation.versionA.authorName}`,
    );
    await expect(widget).toContainText(
      `Version B by ${fixture.reconciliation.versionB.authorName}`,
    );
    await expect(widget.locator('button[data-choice="keep-a"]')).toBeVisible();
    await expect(widget.locator('button[data-choice="keep-b"]')).toBeVisible();
    await expect(
      widget.locator('button[data-choice="keep-both"]'),
    ).toBeVisible();

    await expect(widget).toMatchAriaSnapshot();
    await expect(page).toHaveScreenshot("reconciliation-inline-ui-keep-a.png", {
      fullPage: true,
    });

    await widget.locator('button[data-choice="keep-a"]').click();
    await expect(
      page.locator(`[data-reconciliation-id="${entry.id}"]`),
    ).toHaveCount(0);

    const content = await readEditorText(page, fixture.state.remotePeers ?? []);
    expect(content).toContain(
      normalizeForAssertion(fixture.reconciliation.versionA.content),
    );
    expect(content).not.toContain(
      normalizeForAssertion(fixture.reconciliation.versionB.content),
    );
  });

  test("resolves with Keep Both and merges both versions @smoke", async ({
    page,
  }) => {
    const entry = buildReconciliationEntry(fixture);
    await applyFixtureWithReconciliation(page, fixture, entry);

    await page
      .locator(
        `[data-reconciliation-id="${entry.id}"] button[data-choice="keep-both"]`,
      )
      .click();

    await expect(
      page.locator(`[data-reconciliation-id="${entry.id}"]`),
    ).toHaveCount(0);

    const content = await readEditorText(page, fixture.state.remotePeers ?? []);
    const versionANormalized = normalizeForAssertion(
      fixture.reconciliation.versionA.content,
    );
    const versionBNormalized = normalizeForAssertion(
      fixture.reconciliation.versionB.content,
    );

    expect(content).toContain(versionANormalized);
    expect(content).toContain(versionBNormalized);
    expect(content.indexOf(versionANormalized)).toBeLessThan(
      content.indexOf(versionBNormalized),
    );
  });
});

function buildReconciliationEntry(
  value: ReconciliationFixtureDocument,
): ReconciliationInlineEntry {
  const detector = new ReconciliationDetector({
    thresholdRatio: value.reconciliation.thresholdRatio,
    windowMs: value.reconciliation.windowMs,
  });

  let trigger = null;
  for (const event of value.reconciliation.events) {
    trigger = detector.recordEdit({
      sectionId: value.reconciliation.sectionId,
      authorId: event.authorId,
      timestampMs: event.timestampMs,
      changedChars: event.changedChars,
      sectionLength: event.sectionLength,
    });
  }

  expect(trigger).not.toBeNull();
  expect(trigger?.stats.distinctAuthorCount).toBe(
    value.reconciliation.expectedTrigger.distinctAuthorCount,
  );
  expect(trigger?.stats.changeRatio ?? 0).toBeGreaterThan(
    value.reconciliation.expectedTrigger.changeRatio,
  );

  const from = value.state.docContent.indexOf(
    value.reconciliation.currentContent,
  );
  if (from === -1) {
    throw new Error(
      `missing reconciliation.currentContent in fixture docContent for ${value.name}`,
    );
  }

  const to = from + value.reconciliation.currentContent.length;
  return {
    id: `${value.reconciliation.sectionId}-${trigger?.triggeredAtMs ?? Date.now()}`,
    sectionId: value.reconciliation.sectionId,
    from,
    to,
    versionA: value.reconciliation.versionA,
    versionB: value.reconciliation.versionB,
    triggeredAtMs: trigger?.triggeredAtMs,
  };
}

async function applyFixtureWithReconciliation(
  page: Page,
  value: ReconciliationFixtureDocument,
  entry: ReconciliationInlineEntry,
): Promise<void> {
  await page.goto(value.route);
  await expect(page.getByTestId("document-title")).toBeVisible();
  await expect(
    page.locator('[data-testid="editor-host"] .cm-editor'),
  ).toBeVisible();

  await page.evaluate(
    ({ fixtureState, reconciliationEntry }) => {
      const api = window.__SCRIPTUM_TEST__;
      if (!api) {
        throw new Error("__SCRIPTUM_TEST__ was not installed in fixture mode");
      }

      api.reset();
      if (fixtureState.fixtureName) {
        api.loadFixture(fixtureState.fixtureName);
      }
      api.setDocContent(fixtureState.docContent);
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
      api.setReconciliationEntries([reconciliationEntry]);
    },
    { fixtureState: value.state, reconciliationEntry: entry },
  );
}

async function readEditorText(
  page: Page,
  peers: FixtureRemotePeer[],
): Promise<string> {
  const raw = await page.evaluate(() => {
    const contentRoot = document.querySelector(
      '[data-testid="editor-host"] .cm-content',
    );
    return contentRoot?.textContent ?? "";
  });

  let normalized = raw;
  for (const peer of peers) {
    normalized = normalized.split(peer.name).join("");
  }
  return normalizeForAssertion(normalized);
}

function normalizeForAssertion(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function loadFixture(): ReconciliationFixtureDocument {
  const currentFile = fileURLToPath(import.meta.url);
  const fixturePath = path.resolve(
    path.dirname(currentFile),
    "../fixtures/reconciliation-conflict.json",
  );
  const raw = readFileSync(fixturePath, "utf8");
  return JSON.parse(raw) as ReconciliationFixtureDocument;
}
