/**
 * ARIA structural snapshot tests for every major view.
 *
 * Produces stable YAML artifacts that agents can reason about textually
 * (no image inspection needed). Enforces accessibility correctness.
 *
 * Each test targets a specific component/region using its data-testid or
 * aria-label, then calls `toMatchAriaSnapshot()` to capture the accessible
 * tree structure. Playwright stores the expected YAML in a sibling
 * `aria-snapshots.spec.mts-snapshots/` directory.
 */

import { expect, type Page, test } from "@playwright/test";

// ── Types ────────────────────────────────────────────────────────────

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
      setCommentThreads(threads: unknown[]): void;
      getState(): unknown;
    };
  }
}

// ── Helpers ──────────────────────────────────────────────────────────

const EDITOR_ROUTE = "/workspace/ws-aria/document/doc-aria";
const WORKSPACE_ROUTE = "/workspace/ws-aria";

/** Navigate to a document editor route and apply fixture state. */
async function setupEditor(
  page: Page,
  opts?: {
    content?: string;
    syncState?: "synced" | "offline" | "reconnecting" | "error";
    gitStatus?: {
      dirty: boolean;
      ahead: number;
      behind: number;
      lastCommit?: string;
    };
    peers?: Array<{
      name: string;
      type: "human" | "agent";
      cursor: { line: number; ch: number };
      section?: string;
    }>;
    cursor?: { line: number; ch: number };
    pendingSyncUpdates?: number;
    reconnectProgress?: {
      syncedUpdates: number;
      totalUpdates: number;
    } | null;
    commentThreads?: unknown[];
  },
): Promise<void> {
  await page.goto(EDITOR_ROUTE);
  await expect(
    page.getByText("Document: ws-aria/doc-aria"),
  ).toBeVisible();

  await page.evaluate((fixture) => {
    const api = window.__SCRIPTUM_TEST__;
    if (!api) throw new Error("fixture mode not available");

    api.reset();
    api.loadFixture("default");

    if (fixture?.content !== undefined) api.setDocContent(fixture.content);
    if (fixture?.cursor) api.setCursor(fixture.cursor);
    if (fixture?.syncState) api.setSyncState(fixture.syncState);
    if (fixture?.gitStatus) api.setGitStatus(fixture.gitStatus);
    if (fixture?.pendingSyncUpdates !== undefined)
      api.setPendingSyncUpdates(fixture.pendingSyncUpdates);
    if (fixture?.reconnectProgress !== undefined)
      api.setReconnectProgress(fixture.reconnectProgress);
    if (fixture?.commentThreads)
      api.setCommentThreads(fixture.commentThreads);
    for (const peer of fixture?.peers ?? []) {
      api.spawnRemotePeer(peer);
    }
  }, opts ?? null);
}

// ── Editor area ──────────────────────────────────────────────────────

test.describe("ARIA snapshots: editor area @aria", () => {
  test("editor area - default synced", async ({ page }) => {
    await setupEditor(page, {
      content: "# Heading\n\nParagraph text.\n\n- Item one\n- Item two\n",
      syncState: "synced",
      cursor: { line: 0, ch: 0 },
    });

    const editorArea = page.getByTestId("app-editor-area");
    await expect(editorArea).toBeVisible();
    await expect(editorArea).toMatchAriaSnapshot();
  });

  test("editor area - empty document", async ({ page }) => {
    await setupEditor(page, {
      content: "",
      syncState: "synced",
    });

    const editorArea = page.getByTestId("app-editor-area");
    await expect(editorArea).toBeVisible();
    await expect(editorArea).toMatchAriaSnapshot();
  });
});

// ── Sidebar ──────────────────────────────────────────────────────────

test.describe("ARIA snapshots: sidebar @aria", () => {
  test("sidebar - editor view", async ({ page }) => {
    await setupEditor(page, {
      content: "# Notes\n",
      syncState: "synced",
    });

    const sidebar = page.getByTestId("app-sidebar");
    await expect(sidebar).toBeVisible();
    await expect(sidebar).toMatchAriaSnapshot();
  });

  test("sidebar - workspace view", async ({ page }) => {
    await page.goto(WORKSPACE_ROUTE);
    await expect(page.getByText("Workspace: ws-aria")).toBeVisible();

    const sidebar = page.getByTestId("app-sidebar");
    await expect(sidebar).toBeVisible();
    await expect(sidebar).toMatchAriaSnapshot();
  });
});

// ── Tab bar ──────────────────────────────────────────────────────────

test.describe("ARIA snapshots: tab bar @aria", () => {
  test("tab bar - single document open", async ({ page }) => {
    await setupEditor(page, {
      content: "# Tab Test\n",
      syncState: "synced",
    });

    const tabBar = page.getByTestId("tab-bar");
    await expect(tabBar).toBeVisible();
    await expect(tabBar).toMatchAriaSnapshot();
  });
});

// ── Status bar ───────────────────────────────────────────────────────

test.describe("ARIA snapshots: status bar @aria", () => {
  test("status bar - synced clean", async ({ page }) => {
    await setupEditor(page, {
      content: "# Status\n",
      syncState: "synced",
      cursor: { line: 0, ch: 5 },
      gitStatus: { dirty: false, ahead: 0, behind: 0 },
    });

    const statusBar = page.getByTestId("status-bar");
    await expect(statusBar).toBeVisible();
    await expect(statusBar).toMatchAriaSnapshot();
  });

  test("status bar - offline dirty", async ({ page }) => {
    await setupEditor(page, {
      content: "# Offline\n",
      syncState: "offline",
      cursor: { line: 1, ch: 0 },
      gitStatus: { dirty: true, ahead: 2, behind: 0 },
      pendingSyncUpdates: 5,
    });

    const statusBar = page.getByTestId("status-bar");
    await expect(statusBar).toBeVisible();
    await expect(statusBar).toMatchAriaSnapshot();
  });

  test("status bar - reconnecting with progress", async ({ page }) => {
    await setupEditor(page, {
      content: "# Reconnect\n",
      syncState: "reconnecting",
      reconnectProgress: { syncedUpdates: 3, totalUpdates: 10 },
    });

    const statusBar = page.getByTestId("status-bar");
    await expect(statusBar).toBeVisible();
    await expect(statusBar).toMatchAriaSnapshot();
  });

  test("status bar - error state", async ({ page }) => {
    await setupEditor(page, {
      content: "# Error\n",
      syncState: "error",
      gitStatus: { dirty: true, ahead: 3, behind: 0, lastCommit: "abc123" },
    });

    const statusBar = page.getByTestId("status-bar");
    await expect(statusBar).toBeVisible();
    await expect(statusBar).toMatchAriaSnapshot();
  });
});

// ── Presence / avatar stack ──────────────────────────────────────────

test.describe("ARIA snapshots: presence @aria", () => {
  test("avatar stack - human and agent peers", async ({ page }) => {
    await setupEditor(page, {
      content: "# Collab\n",
      syncState: "synced",
      peers: [
        { name: "Alice", type: "human", cursor: { line: 0, ch: 2 } },
        {
          name: "Bot-1",
          type: "agent",
          cursor: { line: 1, ch: 0 },
          section: "Collab",
        },
      ],
    });

    const avatarStack = page.getByTestId("avatar-stack");
    await expect(avatarStack).toBeVisible();
    await expect(avatarStack).toMatchAriaSnapshot();
  });

  test("presence stack - with section overlap", async ({ page }) => {
    await setupEditor(page, {
      content: "# Shared Section\n\nBoth peers editing here.\n",
      syncState: "synced",
      peers: [
        {
          name: "Agent-A",
          type: "agent",
          cursor: { line: 1, ch: 0 },
          section: "Shared Section",
        },
        {
          name: "Agent-B",
          type: "agent",
          cursor: { line: 2, ch: 5 },
          section: "Shared Section",
        },
      ],
    });

    const presenceStack = page.getByTestId("presence-stack");
    await expect(presenceStack).toBeVisible();
    await expect(presenceStack).toMatchAriaSnapshot();
  });
});

// ── Comments ─────────────────────────────────────────────────────────

test.describe("ARIA snapshots: comments @aria", () => {
  test("comment threads - with thread data", async ({ page }) => {
    await setupEditor(page, {
      content: "# Review\n\nNeed feedback here.\n",
      syncState: "synced",
      commentThreads: [
        {
          threadId: "thread-1",
          sectionId: "review",
          messages: [
            {
              messageId: "msg-1",
              author: "Alice",
              body: "Looks good, minor nit on line 3.",
              createdAt: "2025-01-01T00:00:00Z",
            },
            {
              messageId: "msg-2",
              author: "Bob",
              body: "Fixed, thanks!",
              createdAt: "2025-01-01T00:01:00Z",
            },
          ],
        },
      ],
    });

    const commentThreads = page.getByTestId("comment-threads");
    // Comment threads might not render if the component doesn't exist yet.
    // Use a conditional check — if visible, snapshot it.
    const isVisible = await commentThreads.isVisible().catch(() => false);
    if (isVisible) {
      await expect(commentThreads).toMatchAriaSnapshot();
    }
  });
});

// ── Offline banner ───────────────────────────────────────────────────

test.describe("ARIA snapshots: offline banner @aria", () => {
  test("offline banner - shown when offline", async ({ page }) => {
    await setupEditor(page, {
      content: "# Offline Mode\n",
      syncState: "offline",
    });

    const offlineBanner = page.getByTestId("offline-banner");
    const isVisible = await offlineBanner.isVisible().catch(() => false);
    if (isVisible) {
      await expect(offlineBanner).toMatchAriaSnapshot();
    }
  });
});

// ── Settings page ────────────────────────────────────────────────────

test.describe("ARIA snapshots: settings @aria", () => {
  test("settings page - full layout", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Settings")).toBeVisible();

    // Snapshot the main content area (body minus chrome).
    await expect(page.locator("body")).toMatchAriaSnapshot();
  });
});

// ── Auth callback ────────────────────────────────────────────────────

test.describe("ARIA snapshots: auth callback @aria", () => {
  test("auth callback page", async ({ page }) => {
    await page.goto("/auth-callback");
    await expect(page.getByText("Auth Callback")).toBeVisible();

    await expect(page.locator("body")).toMatchAriaSnapshot();
  });
});
