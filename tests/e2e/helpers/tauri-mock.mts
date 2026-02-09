// Tauri mock injection for e2e tests — simulates window.__TAURI_INTERNALS__
// and @tauri-apps/api/* module imports in a browser context.
// Command names come from the shared contract (tauri-commands.ts).

import type { Page } from "@playwright/test";
import {
  TAURI_AUTH_COMMANDS,
  TAURI_DEEP_LINK_EVENT,
  TAURI_REDIRECT_URI,
} from "../../../packages/web/src/lib/tauri-commands.ts";

/** Contract values passed to the browser via addInitScript. */
interface TauriMockContract {
  commands: typeof TAURI_AUTH_COMMANDS;
  deepLinkEvent: string;
  redirectUri: string;
}

/**
 * Inject Tauri mock globals via addInitScript. This runs before any app code
 * and sets up __TAURI_INTERNALS__, __TAURI_INVOKE__, __TAURI_LISTEN__, and
 * the __TAURI_MOCK__ namespace for test assertions.
 *
 * Command names are derived from the shared contract — not hardcoded strings.
 */
export async function injectTauriMock(page: Page): Promise<void> {
  const contract: TauriMockContract = {
    commands: TAURI_AUTH_COMMANDS,
    deepLinkEvent: TAURI_DEEP_LINK_EVENT,
    redirectUri: TAURI_REDIRECT_URI,
  };

  await page.addInitScript((c: TauriMockContract) => {
    // Command handlers map — tests can override individual commands.
    const commandHandlers: Record<
      string,
      (args: Record<string, unknown>) => unknown
    > = {
      [c.commands.AUTH_REDIRECT_URI]: () => c.redirectUri,

      [c.commands.AUTH_OPEN_BROWSER]: (args) => {
        (window as unknown as Record<string, unknown>).__TAURI_MOCK__ = {
          ...((window as unknown as Record<string, unknown>).__TAURI_MOCK__ as
            | Record<string, unknown>
            | undefined),
          _lastBrowserUrl: args.authorizationUrl ?? args.url,
        };
        return undefined;
      },

      [c.commands.AUTH_PARSE_CALLBACK]: (args) => {
        const url = new URL(args.url as string);
        return {
          url: args.url,
          code: url.searchParams.get("code"),
          state: url.searchParams.get("state"),
          error: url.searchParams.get("error"),
          error_description: url.searchParams.get("error_description"),
        };
      },

      [c.commands.AUTH_STORE_TOKENS]: (args) => {
        const mock = (window as unknown as Record<string, unknown>)
          .__TAURI_MOCK__ as Record<string, unknown>;
        const keychain =
          (mock._keychain as Map<string, unknown>) ??
          new Map<string, unknown>();
        keychain.set("tokens", args.tokens);
        mock._keychain = keychain;
        return undefined;
      },

      [c.commands.AUTH_LOAD_TOKENS]: () => {
        const mock = (window as unknown as Record<string, unknown>)
          .__TAURI_MOCK__ as Record<string, unknown> | undefined;
        const keychain = mock?._keychain as Map<string, unknown> | undefined;
        return keychain?.get("tokens") ?? null;
      },

      [c.commands.AUTH_CLEAR_TOKENS]: () => {
        const mock = (window as unknown as Record<string, unknown>)
          .__TAURI_MOCK__ as Record<string, unknown> | undefined;
        const keychain = mock?._keychain as Map<string, unknown> | undefined;
        keychain?.delete("tokens");
        return undefined;
      },
    };

    // Event listeners keyed by event name
    const eventListeners: Record<
      string,
      Array<(event: { payload: unknown }) => void>
    > = {};

    // Set up __TAURI_INTERNALS__ — Tauri 2.0 detection marker
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};

    // Set up mock namespace for test assertions
    (window as unknown as Record<string, unknown>).__TAURI_MOCK__ = {
      _lastBrowserUrl: null as string | null,
      _keychain: new Map<string, unknown>(),
      _commandHandlers: commandHandlers,
      _eventListeners: eventListeners,

      /** Override a command handler from a test. */
      setCommandHandler: (
        command: string,
        handler: (args: Record<string, unknown>) => unknown,
      ) => {
        commandHandlers[command] = handler;
      },

      /** Emit a Tauri event (simulates deep link, etc.). */
      emit: (event: string, payload: unknown) => {
        const listeners = eventListeners[event] ?? [];
        for (const listener of listeners) {
          listener({ payload });
        }
      },
    };

    // Global invoke function — delegates to command handlers.
    (window as unknown as Record<string, unknown>).__TAURI_INVOKE__ = async (
      command: string,
      args: Record<string, unknown> = {},
    ) => {
      const handler = commandHandlers[command];
      if (!handler) {
        throw new Error(`Tauri mock: unknown command "${command}"`);
      }
      return handler(args);
    };

    // Global listen function — registers event listeners.
    (window as unknown as Record<string, unknown>).__TAURI_LISTEN__ = async (
      event: string,
      handler: (event: { payload: unknown }) => void,
    ) => {
      if (!eventListeners[event]) {
        eventListeners[event] = [];
      }
      eventListeners[event].push(handler);
      // Return unlisten function
      return () => {
        const list = eventListeners[event];
        if (list) {
          const idx = list.indexOf(handler);
          if (idx >= 0) list.splice(idx, 1);
        }
      };
    };
  }, contract);

  // Intercept HTML navigation responses to inject import map for @tauri-apps/api/*
  await page.route("**/*", async (route) => {
    // Only intercept document (navigation) requests — they're the only ones
    // that return HTML where we need to inject the import map. Let all other
    // requests (XHR, fetch, scripts, images) pass through normally so relay
    // API calls and other resources aren't disrupted.
    if (route.request().resourceType() !== "document") {
      await route.continue();
      return;
    }

    let response: Awaited<ReturnType<typeof route.fetch>> | undefined;
    try {
      response = await route.fetch();
    } catch {
      // If the fetch fails (e.g., dev server not ready), let the browser handle it.
      await route.continue();
      return;
    }
    const contentType = response.headers()["content-type"] ?? "";

    if (!contentType.includes("text/html")) {
      await route.fulfill({ response });
      return;
    }

    let body = await response.text();

    // Inject import map for @tauri-apps/api/core and @tauri-apps/api/event
    const importMap = `
<script type="importmap">
{
  "imports": {
    "@tauri-apps/api/core": "data:text/javascript;charset=utf-8,${encodeURIComponent(`
      export async function invoke(command, args = {}) {
        const fn = window.__TAURI_INVOKE__;
        if (!fn) throw new Error('Tauri mock not initialized');
        return fn(command, args);
      }
    `)}",
    "@tauri-apps/api/event": "data:text/javascript;charset=utf-8,${encodeURIComponent(`
      export async function listen(event, handler) {
        const fn = window.__TAURI_LISTEN__;
        if (!fn) throw new Error('Tauri mock not initialized');
        return fn(event, handler);
      }
    `)}"
  }
}
</script>`;

    // Insert import map at the start of <head> (case-insensitive, first match only)
    body = body.replace(/<head>/i, `<head>${importMap}`);

    await route.fulfill({
      response,
      body,
      headers: {
        ...response.headers(),
        "content-type": "text/html; charset=utf-8",
      },
    });
  });
}

/**
 * Emit a deep link event into the Tauri mock.
 * Simulates the OS handing `scriptum://auth/callback?code=X&state=Y` to the app.
 */
export async function emitDeepLink(page: Page, url: string): Promise<void> {
  await page.evaluate(
    ({ callbackUrl, event }) => {
      const mock = (window as unknown as Record<string, unknown>)
        .__TAURI_MOCK__ as Record<string, (...args: unknown[]) => void>;
      mock.emit(event, [callbackUrl]);
    },
    { callbackUrl: url, event: TAURI_DEEP_LINK_EVENT },
  );
}

/**
 * Override a Tauri command handler from the test.
 */
export async function overrideTauriCommand(
  page: Page,
  command: string,
  handlerBody: string,
): Promise<void> {
  await page.evaluate(
    ({ command, handlerBody }) => {
      const mock = (window as unknown as Record<string, unknown>)
        .__TAURI_MOCK__ as Record<
        string,
        (cmd: string, fn: (args: Record<string, unknown>) => unknown) => void
      >;
      // eslint-disable-next-line no-new-func
      const fn = new Function("args", handlerBody) as (
        args: Record<string, unknown>,
      ) => unknown;
      mock.setCommandHandler(command, fn);
    },
    { command, handlerBody },
  );
}

/** Read the last browser URL opened by the mock. */
export async function getLastBrowserUrl(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const mock = (window as unknown as Record<string, unknown>)
      .__TAURI_MOCK__ as Record<string, unknown> | undefined;
    return (mock?._lastBrowserUrl as string) ?? null;
  });
}

/** Check if the keychain has stored tokens. */
export async function getKeychainTokens(page: Page): Promise<unknown> {
  return page.evaluate(() => {
    const mock = (window as unknown as Record<string, unknown>)
      .__TAURI_MOCK__ as Record<string, unknown> | undefined;
    const keychain = mock?._keychain as Map<string, unknown> | undefined;
    return keychain?.get("tokens") ?? null;
  });
}

/** Re-export command constants for use in test specs. */
export { TAURI_AUTH_COMMANDS } from "../../../packages/web/src/lib/tauri-commands.ts";
