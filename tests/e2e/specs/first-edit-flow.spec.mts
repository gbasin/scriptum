import { type ChildProcess, spawn } from "node:child_process";
import { once } from "node:events";
import net from "node:net";
import { expect, test } from "@playwright/test";

const LOCAL_DAEMON_HOST = "127.0.0.1";
const LOCAL_DAEMON_PORT = 39091;
const FIRST_EDIT_TEXT = "First-run edit persists through daemon-backed CRDT.";

interface RunningDaemon {
  child: ChildProcess | null;
  output: string[];
  owned: boolean;
}

test.describe("first edit flow e2e @smoke", () => {
  test("creates workspace and first document, then persists edits @smoke", async ({
    browser,
    page,
  }) => {
    const daemon = await ensureDaemonReady();

    try {
      await page.goto("/");
      await expect(
        page.getByTestId("index-create-workspace-button"),
      ).toBeVisible({ timeout: 20_000 });

      await page.getByTestId("index-create-workspace-button").click();
      await expect(page.getByTestId("workspace-route")).toBeVisible();

      const workspaceId = activeWorkspaceIdFromUrl(page.url());
      await expect(page.getByTestId(`workspace-${workspaceId}`)).toBeVisible();

      await page.getByTestId("workspace-create-first-document").click();
      await expect(page.getByTestId("document-title")).toBeVisible();
      await expect(page.getByTestId("status-bar")).toContainText(
        "Sync: Synced",
        {
          timeout: 20_000,
        },
      );

      const documentUrl = page.url();
      const editor = page.locator('[data-testid="editor-host"] .cm-content');
      await editor.click();
      await page.keyboard.type(FIRST_EDIT_TEXT);
      await expect(editor).toContainText(FIRST_EDIT_TEXT, { timeout: 20_000 });

      await page.reload();
      await expect(page).toHaveURL(documentUrl);
      await expect(
        page.locator('[data-testid="editor-host"] .cm-content'),
      ).toContainText(FIRST_EDIT_TEXT, { timeout: 20_000 });

      const secondContext = await browser.newContext();
      try {
        const secondPage = await secondContext.newPage();
        await secondPage.goto(documentUrl);
        await expect(
          secondPage.locator('[data-testid="editor-host"] .cm-content'),
        ).toContainText(FIRST_EDIT_TEXT, { timeout: 20_000 });
      } finally {
        await secondContext.close();
      }
    } finally {
      await stopDaemon(daemon);
    }
  });
});

function activeWorkspaceIdFromUrl(url: string): string {
  const pathname = new URL(url).pathname;
  const match = /^\/workspace\/([^/]+)/.exec(pathname);
  if (!match) {
    throw new Error(`unable to resolve workspace id from URL: ${url}`);
  }
  return decodeURIComponent(match[1]);
}

async function ensureDaemonReady(): Promise<RunningDaemon> {
  const alreadyRunning = await canConnectToPort(
    LOCAL_DAEMON_HOST,
    LOCAL_DAEMON_PORT,
  );
  if (alreadyRunning) {
    return {
      child: null,
      output: [],
      owned: false,
    };
  }

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

  await Promise.race([
    waitForTcpPort({
      host: LOCAL_DAEMON_HOST,
      port: LOCAL_DAEMON_PORT,
      timeoutMs: 120_000,
    }),
    once(child, "exit").then(([code, signal]) => {
      throw new Error(
        `scriptum-daemon exited before websocket became ready (code=${String(code)} signal=${String(signal)}): ${output.join("")}`,
      );
    }),
  ]);

  return {
    child,
    output,
    owned: true,
  };
}

async function stopDaemon(running: RunningDaemon): Promise<void> {
  if (!running.owned || !running.child) {
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
