// Tauri auth bridge — desktop OAuth flow via system browser + deep links.
// All @tauri-apps imports are dynamic so this module is safe to import in browser builds.
// Command names and payload types come from tauri-commands.ts (shared contract).

import type { AuthService, AuthSession, StartGitHubOAuthResult } from "./auth";
import {
  TAURI_AUTH_COMMANDS,
  TAURI_DEEP_LINK_EVENT,
  type TauriAuthTokens,
  type TauriOAuthCallbackPayload,
} from "./tauri-commands";

const DEFAULT_TIMEOUT_MS = 120_000;

/** Synchronous Tauri 2.0 detection (available before any async import). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

interface TauriInvokeApi {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

interface TauriEventApi {
  listen: <T>(
    event: string,
    handler: (event: { payload: T }) => void,
  ) => Promise<() => void>;
}

// Module specifiers as variables — prevents Vite's static import analysis from
// trying to resolve these at build time when the packages are absent (browser build).
const TAURI_CORE = "@tauri-apps/api/core";
const TAURI_EVENT = "@tauri-apps/api/event";

async function getTauriCore(): Promise<TauriInvokeApi> {
  return (await import(/* @vite-ignore */ TAURI_CORE)) as TauriInvokeApi;
}

async function getTauriEvent(): Promise<TauriEventApi> {
  return (await import(/* @vite-ignore */ TAURI_EVENT)) as TauriEventApi;
}

/** Get the desktop OAuth redirect URI (`scriptum://auth/callback`). */
export async function getTauriRedirectUri(): Promise<string> {
  const { invoke } = await getTauriCore();
  return invoke<string>(TAURI_AUTH_COMMANDS.AUTH_REDIRECT_URI);
}

/** Open a URL in the system browser (not the webview). */
export async function openInSystemBrowser(url: string): Promise<void> {
  const { invoke } = await getTauriCore();
  await invoke(TAURI_AUTH_COMMANDS.AUTH_OPEN_BROWSER, {
    authorizationUrl: url,
  });
}

interface DeepLinkListenerResult {
  promise: Promise<{ code: string; state: string }>;
  cancel: () => void;
}

/**
 * Listen for a deep-link callback containing OAuth code + state.
 * Returns a cancellable promise that resolves with { code, state }.
 */
export function listenForDeepLinkCallback(options?: {
  timeoutMs?: number;
}): DeepLinkListenerResult {
  const timeoutMs = options?.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  let cancelled = false;
  let unlisten: (() => void) | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let settled = false;

  const promise = new Promise<{ code: string; state: string }>(
    (resolve, reject) => {
      const safeReject = (err: Error) => {
        if (settled) return;
        settled = true;
        reject(err);
      };
      const safeResolve = (val: { code: string; state: string }) => {
        if (settled) return;
        settled = true;
        resolve(val);
      };

      const cleanup = () => {
        if (timer !== null) {
          clearTimeout(timer);
          timer = null;
        }
        unlisten?.();
      };

      timer = setTimeout(() => {
        cleanup();
        safeReject(new Error("Deep link callback timed out"));
      }, timeoutMs);

      (async () => {
        const eventApi = await getTauriEvent();
        const coreApi = await getTauriCore();

        if (cancelled) {
          cleanup();
          // Don't reject — caller already moved on.
          return;
        }

        unlisten = await eventApi.listen<string[]>(
          TAURI_DEEP_LINK_EVENT,
          async (event) => {
            try {
              const urls = event.payload;
              if (!urls || urls.length === 0) return;

              const payload =
                await coreApi.invoke<TauriOAuthCallbackPayload>(
                  TAURI_AUTH_COMMANDS.AUTH_PARSE_CALLBACK,
                  { url: urls[0] },
                );

              if (payload.error) {
                cleanup();
                safeReject(
                  new Error(payload.error_description ?? payload.error),
                );
                return;
              }

              if (!payload.code || !payload.state) {
                cleanup();
                safeReject(new Error("Deep link missing code or state"));
                return;
              }

              cleanup();
              safeResolve({ code: payload.code, state: payload.state });
            } catch (err) {
              cleanup();
              safeReject(err instanceof Error ? err : new Error(String(err)));
            }
          },
        );
      })().catch((err) => {
        cleanup();
        safeReject(err instanceof Error ? err : new Error(String(err)));
      });
    },
  );

  return {
    promise,
    cancel: () => {
      cancelled = true;
      unlisten?.();
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
    },
  };
}

/** Store tokens in the system keychain. */
export async function storeTauriTokens(session: AuthSession): Promise<void> {
  const { invoke } = await getTauriCore();
  const tokens: TauriAuthTokens = {
    access_token: session.accessToken,
    refresh_token: session.refreshToken,
    access_expires_at: session.accessExpiresAt,
    refresh_expires_at: session.refreshExpiresAt,
  };
  await invoke(TAURI_AUTH_COMMANDS.AUTH_STORE_TOKENS, { tokens });
}

/** Load tokens from the system keychain. */
export async function loadTauriTokens(): Promise<TauriAuthTokens | null> {
  const { invoke } = await getTauriCore();
  return invoke<TauriAuthTokens | null>(TAURI_AUTH_COMMANDS.AUTH_LOAD_TOKENS);
}

/** Clear tokens from the system keychain. */
export async function clearTauriTokens(): Promise<void> {
  const { invoke } = await getTauriCore();
  await invoke(TAURI_AUTH_COMMANDS.AUTH_CLEAR_TOKENS);
}

/**
 * Full desktop login orchestrator.
 * 1. Gets redirect URI from Tauri command
 * 2. Sets up deep link listener
 * 3. Starts OAuth via auth service (redirect: false)
 * 4. Opens system browser with authorization URL
 * 5. Awaits deep link callback
 * 6. Exchanges code for session via auth service
 * 7. Stores tokens in keychain as bonus persistence
 */
export async function performTauriLogin(
  auth: Pick<AuthService, "startGitHubOAuth" | "handleOAuthCallback">,
): Promise<AuthSession> {
  const redirectUri = await getTauriRedirectUri();
  const listener = listenForDeepLinkCallback();

  // Suppress unhandled rejection if we cancel the listener before it settles.
  listener.promise.catch(() => {});

  let startResult: StartGitHubOAuthResult;
  try {
    startResult = await auth.startGitHubOAuth({
      redirectUri,
      redirect: false,
    });
  } catch (err) {
    listener.cancel();
    throw err;
  }

  try {
    await openInSystemBrowser(startResult.authorizationUrl);
  } catch (err) {
    listener.cancel();
    throw err;
  }

  let callbackParams: { code: string; state: string };
  try {
    callbackParams = await listener.promise;
  } catch (err) {
    listener.cancel();
    throw err;
  }

  const session = await auth.handleOAuthCallback({
    searchParams: new URLSearchParams(callbackParams),
  });

  // Best-effort keychain persistence — don't fail login if this errors.
  try {
    await storeTauriTokens(session);
  } catch {
    // Keychain storage is a bonus; localStorage still works in the webview.
  }

  return session;
}
