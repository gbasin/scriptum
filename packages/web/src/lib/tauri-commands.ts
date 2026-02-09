// Tauri IPC command contract — shared between tauri-auth.ts and e2e mocks.
// Must stay in sync with packages/desktop/tauri-auth-contract.json.
// See tauri-commands.test.ts for automated validation.

/** Auth-related Tauri command names. */
export const TAURI_AUTH_COMMANDS = {
  AUTH_REDIRECT_URI: "auth_redirect_uri",
  AUTH_OPEN_BROWSER: "auth_open_browser",
  AUTH_PARSE_CALLBACK: "auth_parse_callback",
  AUTH_STORE_TOKENS: "auth_store_tokens",
  AUTH_LOAD_TOKENS: "auth_load_tokens",
  AUTH_CLEAR_TOKENS: "auth_clear_tokens",
} as const;

export type TauriAuthCommand =
  (typeof TAURI_AUTH_COMMANDS)[keyof typeof TAURI_AUTH_COMMANDS];

/** Deep link event name emitted by the OS when handling scriptum:// URLs. */
export const TAURI_DEEP_LINK_EVENT = "scriptum://auth/deep-link";

/** Desktop OAuth redirect URI (custom scheme handled by Tauri). */
export const TAURI_REDIRECT_URI = "scriptum://auth/callback";

/** Token payload shape — matches Rust AuthTokens (serde snake_case). */
export interface TauriAuthTokens {
  access_token: string;
  refresh_token: string;
  access_expires_at: string | null;
  refresh_expires_at: string | null;
}

/** OAuth callback payload — matches Rust OAuthCallbackPayload. */
export interface TauriOAuthCallbackPayload {
  url: string;
  code: string | null;
  state: string | null;
  error: string | null;
  error_description: string | null;
}
