import { expect, test } from "@playwright/test";

interface DesktopMenuShortcutContract {
  action: string;
  menuId: string;
  accelerator: string;
}

interface DesktopWebDriverContract {
  menuActionEvent: string;
  importDialogSelectedEvent: string;
  exportDialogSelectedEvent: string;
  daemonIpcEntrypoint: string;
  fileWatcherIntegration: string;
  menuShortcuts: DesktopMenuShortcutContract[];
}

interface WebDriverErrorValue {
  error?: string;
  message?: string;
}

interface WebDriverExecuteResult<T> {
  ok: boolean;
  result?: T;
  error?: string;
}

const webdriverBaseUrl = (
  process.env.SCRIPTUM_DESKTOP_WEBDRIVER_URL ?? ""
).replace(/\/+$/, "");
const rawCapabilities =
  process.env.SCRIPTUM_DESKTOP_WEBDRIVER_CAPABILITIES_JSON ?? "";
const isMacOs = process.platform === "darwin";
const hasDesktopWebDriverConfig =
  webdriverBaseUrl.length > 0 && rawCapabilities.length > 0;

const INVOKE_SCRIPT = String.raw`
const commandName = arguments[0];
const commandArgs = arguments[1] ?? {};
const done = arguments[arguments.length - 1];

const invoke =
  window.__TAURI__?.core?.invoke ??
  window.__TAURI_INTERNALS__?.invoke ??
  window.__TAURI_INTERNALS__?.tauri?.invoke ??
  null;

if (!invoke) {
  done({ ok: false, error: "tauri invoke bridge unavailable" });
  return;
}

Promise.resolve()
  .then(() => invoke(commandName, commandArgs))
  .then((result) => done({ ok: true, result }))
  .catch((error) => done({ ok: false, error: String(error) }));
`;

function parseCapabilities() {
  try {
    return JSON.parse(rawCapabilities) as Record<string, unknown>;
  } catch (error) {
    throw new Error(
      `Invalid SCRIPTUM_DESKTOP_WEBDRIVER_CAPABILITIES_JSON: ${String(error)}`,
    );
  }
}

function errorFromResponse(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") {
    return null;
  }

  const value = payload as { value?: WebDriverErrorValue };
  if (!value.value || typeof value.value !== "object") {
    return null;
  }

  if (!value.value.error) {
    return null;
  }

  return `${value.value.error}: ${value.value.message ?? "unknown webdriver failure"}`;
}

async function requestWebDriver(
  method: string,
  url: string,
  body?: unknown,
): Promise<unknown> {
  const response = await fetch(url, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  const payload = text.length > 0 ? (JSON.parse(text) as unknown) : {};
  const webdriverError = errorFromResponse(payload);
  if (!response.ok || webdriverError) {
    throw new Error(
      `WebDriver request failed (${method} ${url})${webdriverError ? `: ${webdriverError}` : ""}`,
    );
  }
  return payload;
}

async function createSession(
  baseUrl: string,
  capabilities: Record<string, unknown>,
): Promise<string> {
  const payload = await requestWebDriver("POST", `${baseUrl}/session`, {
    capabilities,
  });
  const response = payload as {
    sessionId?: string;
    value?: { sessionId?: string };
  };
  const sessionId = response.value?.sessionId ?? response.sessionId;
  if (!sessionId) {
    throw new Error("WebDriver did not return a session id");
  }
  return sessionId;
}

async function executeAsyncScript<T>(
  baseUrl: string,
  sessionId: string,
  script: string,
  args: unknown[] = [],
): Promise<T> {
  const payload = await requestWebDriver(
    "POST",
    `${baseUrl}/session/${sessionId}/execute/async`,
    {
      script,
      args,
    },
  );

  const response = payload as { value?: T };
  return response.value as T;
}

async function deleteSession(
  baseUrl: string,
  sessionId: string,
): Promise<void> {
  await requestWebDriver("DELETE", `${baseUrl}/session/${sessionId}`);
}

async function invokeTauriCommand<T>(
  baseUrl: string,
  sessionId: string,
  commandName: string,
  commandArgs: Record<string, unknown> = {},
): Promise<T> {
  const response = await executeAsyncScript<WebDriverExecuteResult<T>>(
    baseUrl,
    sessionId,
    INVOKE_SCRIPT,
    [commandName, commandArgs],
  );

  if (!response.ok) {
    throw new Error(
      `Tauri invoke for ${commandName} failed: ${response.error ?? "unknown"}`,
    );
  }

  return response.result as T;
}

test.describe("desktop tauri webdriver contract @desktop", () => {
  test.skip(
    isMacOs,
    "macOS desktop visual tests are excluded (WKWebView has no WebDriver support)",
  );
  test.skip(
    !hasDesktopWebDriverConfig,
    "Set SCRIPTUM_DESKTOP_WEBDRIVER_URL and SCRIPTUM_DESKTOP_WEBDRIVER_CAPABILITIES_JSON",
  );

  let sessionId = "";
  let capabilities: Record<string, unknown>;

  test.beforeAll(async () => {
    capabilities = parseCapabilities();
    sessionId = await createSession(webdriverBaseUrl, capabilities);
  });

  test.afterAll(async () => {
    if (sessionId.length > 0) {
      await deleteSession(webdriverBaseUrl, sessionId);
    }
  });

  test("daemon IPC wiring responds through tauri invoke bridge", async () => {
    const greeting = await invokeTauriCommand<string>(
      webdriverBaseUrl,
      sessionId,
      "greet",
      { name: "Desktop WebDriver" },
    );
    expect(greeting).toContain("Hello, Desktop WebDriver!");

    const redirectUri = await invokeTauriCommand<string>(
      webdriverBaseUrl,
      sessionId,
      "auth_redirect_uri",
    );
    expect(redirectUri).toBe("scriptum://auth/callback");
  });

  test("file dialog channels are stable in desktop test contract", async () => {
    const contract = await invokeTauriCommand<DesktopWebDriverContract>(
      webdriverBaseUrl,
      sessionId,
      "desktop_webdriver_contract",
    );

    expect(contract.importDialogSelectedEvent).toBe(
      "scriptum://dialog/import-selected",
    );
    expect(contract.exportDialogSelectedEvent).toBe(
      "scriptum://dialog/export-selected",
    );
  });

  test("window and menu shortcuts remain stable in desktop test contract", async () => {
    const contract = await invokeTauriCommand<DesktopWebDriverContract>(
      webdriverBaseUrl,
      sessionId,
      "desktop_webdriver_contract",
    );

    expect(contract.menuActionEvent).toBe("scriptum://menu-action");
    expect(contract.menuShortcuts).toEqual(
      expect.arrayContaining([
        {
          action: "new-document",
          menuId: "menu.new-document",
          accelerator: "CmdOrCtrl+N",
        },
        {
          action: "save-document",
          menuId: "menu.save-document",
          accelerator: "CmdOrCtrl+S",
        },
        {
          action: "close-window",
          menuId: "menu.close-window",
          accelerator: "CmdOrCtrl+W",
        },
        {
          action: "quit-app",
          menuId: "menu.quit-app",
          accelerator: "CmdOrCtrl+Q",
        },
      ]),
    );
  });

  test("file watcher integration surface is exposed by desktop contract", async () => {
    const contract = await invokeTauriCommand<DesktopWebDriverContract>(
      webdriverBaseUrl,
      sessionId,
      "desktop_webdriver_contract",
    );
    expect(contract.daemonIpcEntrypoint).toContain("start_embedded");
    expect(contract.fileWatcherIntegration).toContain("watcher");

    const traySnapshot = await invokeTauriCommand<{
      status: string;
      pendingChanges: number;
    }>(webdriverBaseUrl, sessionId, "tray_get_sync_status");
    expect(typeof traySnapshot.status).toBe("string");
    expect(typeof traySnapshot.pendingChanges).toBe("number");
  });
});
