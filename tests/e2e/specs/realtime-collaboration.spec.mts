import { type ChildProcess, spawn } from "node:child_process";
import { once } from "node:events";
import net from "node:net";
import { expect, type Page, test } from "@playwright/test";

const COLLAB_ROUTE = "/workspace/ws-realtime/document/doc-realtime";
const LOCAL_DAEMON_YJS_HOST = "127.0.0.1";
const LOCAL_DAEMON_YJS_PORT = 39091;
const FIRST_EDIT_TEXT = "Real-time update from page A.";
const SECOND_EDIT_TEXT = " Peer B ack.";

interface BootstrapIdentity {
  userId: string;
  userName: string;
}

interface RunningDaemon {
  child: ChildProcess;
  output: string[];
}

test.describe("realtime collaboration integration @smoke", () => {
  test("syncs edits across two pages over websocket and transitions sync state @smoke", async ({
    browser,
  }) => {
    const contextA = await browser.newContext();
    const contextB = await browser.newContext();
    const pageA = await contextA.newPage();
    const pageB = await contextB.newPage();

    let daemon: RunningDaemon | null = null;
    try {
      await bootstrapAuthenticatedRoute(pageA, {
        userId: "collab-a",
        userName: "Collab A",
      });
      await bootstrapAuthenticatedRoute(pageB, {
        userId: "collab-b",
        userName: "Collab B",
      });
      await expect(pageA.getByTestId("document-title")).toBeVisible();
      await expect(pageB.getByTestId("document-title")).toBeVisible();
      await expect(
        pageA.locator('[data-testid="editor-host"] .cm-editor'),
      ).toBeVisible();
      await expect(
        pageB.locator('[data-testid="editor-host"] .cm-editor'),
      ).toBeVisible();

      const statusA = pageA.getByTestId("status-bar");
      const statusB = pageB.getByTestId("status-bar");
      await expect(statusA).toContainText("Sync: Reconnecting");
      await expect(statusB).toContainText("Sync: Reconnecting");

      daemon = startDaemon();
      await waitForDaemonReady(daemon);

      await expect(statusA).toContainText("Sync: Synced", { timeout: 20_000 });
      await expect(statusB).toContainText("Sync: Synced", { timeout: 20_000 });

      const editorA = pageA.locator('[data-testid="editor-host"] .cm-content');
      const editorB = pageB.locator('[data-testid="editor-host"] .cm-content');

      await editorA.click();
      await pageA.keyboard.type(FIRST_EDIT_TEXT);
      await expect(editorB).toContainText(FIRST_EDIT_TEXT, { timeout: 20_000 });

      await editorB.click();
      await pageB.keyboard.type(SECOND_EDIT_TEXT);
      await expect(editorA).toContainText(SECOND_EDIT_TEXT, {
        timeout: 20_000,
      });

      await expect
        .poll(async () => pageA.locator(".cm-remote-cursor").count(), {
          timeout: 20_000,
        })
        .toBeGreaterThan(0);
      await expect
        .poll(async () => pageB.locator(".cm-remote-cursor").count(), {
          timeout: 20_000,
        })
        .toBeGreaterThan(0);
    } finally {
      await stopDaemon(daemon);
      await contextA.close();
      await contextB.close();
    }
  });
});

async function bootstrapAuthenticatedRoute(
  page: Page,
  identity: BootstrapIdentity,
): Promise<void> {
  await page.goto(COLLAB_ROUTE);

  await page.evaluate(async ({ userId, userName }) => {
    const [{ useAuthStore }, { useWorkspaceStore }, { useDocumentsStore }] =
      await Promise.all([
        import("/src/store/auth.ts"),
        import("/src/store/workspace.ts"),
        import("/src/store/documents.ts"),
      ]);

    const nowIso = new Date(Date.now()).toISOString();
    useAuthStore.setState({
      status: "authenticated",
      user: {
        id: userId,
        email: `${userId}@example.test`,
        display_name: userName,
      },
      accessToken: `${userId}-access-token`,
      accessExpiresAt: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
      refreshToken: `${userId}-refresh-token`,
      refreshExpiresAt: new Date(
        Date.now() + 24 * 60 * 60 * 1000,
      ).toISOString(),
      error: null,
    });

    useWorkspaceStore.getState().upsertWorkspace({
      id: "ws-realtime",
      slug: "ws-realtime",
      name: "Realtime Workspace",
      role: "owner",
      createdAt: nowIso,
      updatedAt: nowIso,
      etag: "ws-realtime-etag",
    });
    useWorkspaceStore.getState().setActiveWorkspaceId("ws-realtime");

    useDocumentsStore.getState().setDocuments([
      {
        id: "doc-realtime",
        workspaceId: "ws-realtime",
        path: "notes/realtime.md",
        title: "realtime.md",
        bodyMd: "",
        tags: [],
        headSeq: 0,
        etag: "doc-realtime-etag",
        archivedAt: null,
        deletedAt: null,
        createdAt: nowIso,
        updatedAt: nowIso,
      },
    ]);
    useDocumentsStore
      .getState()
      .setActiveDocumentForWorkspace("ws-realtime", "doc-realtime");
  }, identity);

  await expect(page.getByTestId("document-title")).toBeVisible();
}

function startDaemon(): RunningDaemon {
  const output: string[] = [];
  const child = spawn("cargo", ["run", "-p", "scriptum-daemon"], {
    cwd: process.cwd(),
    env: {
      ...process.env,
      RUST_LOG: process.env.RUST_LOG ?? "warn",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  child.stdout?.on("data", (chunk) => {
    output.push(String(chunk));
  });
  child.stderr?.on("data", (chunk) => {
    output.push(String(chunk));
  });

  return { child, output };
}

async function waitForDaemonReady(running: RunningDaemon): Promise<void> {
  await Promise.race([
    waitForTcpPort({
      host: LOCAL_DAEMON_YJS_HOST,
      port: LOCAL_DAEMON_YJS_PORT,
      timeoutMs: 20_000,
    }),
    once(running.child, "exit").then(([code, signal]) => {
      throw new Error(
        `scriptum-daemon exited before websocket became ready (code=${String(code)} signal=${String(signal)}): ${running.output.join("")}`,
      );
    }),
  ]);
}

async function stopDaemon(running: RunningDaemon | null): Promise<void> {
  if (!running) {
    return;
  }

  if (running.child.exitCode !== null || running.child.signalCode !== null) {
    return;
  }

  running.child.kill("SIGTERM");
  const exited = await Promise.race([
    once(running.child, "exit").then(() => true),
    delay(5_000).then(() => false),
  ]);

  if (!exited && running.child.exitCode === null) {
    running.child.kill("SIGKILL");
    await once(running.child, "exit");
  }
}

async function waitForTcpPort(options: {
  host: string;
  port: number;
  timeoutMs: number;
}): Promise<void> {
  const startedAt = Date.now();
  while (Date.now() - startedAt < options.timeoutMs) {
    const connected = await canConnectToPort(options.host, options.port);
    if (connected) {
      return;
    }
    await delay(100);
  }

  throw new Error(
    `timed out waiting for TCP ${options.host}:${options.port} after ${options.timeoutMs}ms`,
  );
}

function canConnectToPort(host: string, port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = net.createConnection({ host, port });

    socket.once("connect", () => {
      socket.destroy();
      resolve(true);
    });

    socket.once("error", () => {
      socket.destroy();
      resolve(false);
    });
  });
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}
