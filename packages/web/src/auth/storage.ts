// localStorage keys and helpers for auth session persistence.

const STORAGE_KEY_ACCESS_TOKEN = "scriptum:access_token";
const STORAGE_KEY_ACCESS_EXPIRES_AT = "scriptum:access_expires_at";
const STORAGE_KEY_REFRESH_TOKEN = "scriptum:refresh_token";
const STORAGE_KEY_REFRESH_EXPIRES_AT = "scriptum:refresh_expires_at";
const STORAGE_KEY_USER = "scriptum:user";
const STORAGE_KEY_OAUTH_STATE = "scriptum:oauth_state";
const STORAGE_KEY_CODE_VERIFIER = "scriptum:code_verifier";
const STORAGE_KEY_FLOW_ID = "scriptum:flow_id";

export interface StoredSession {
  accessToken: string;
  accessExpiresAt: string;
  refreshToken: string;
  refreshExpiresAt: string;
  user: { id: string; email: string; display_name: string };
}

export interface StoredOAuthFlow {
  state: string;
  codeVerifier: string;
  flowId: string;
}

/** Save session tokens + user to localStorage. */
export function saveSession(session: StoredSession): void {
  localStorage.setItem(STORAGE_KEY_ACCESS_TOKEN, session.accessToken);
  localStorage.setItem(STORAGE_KEY_ACCESS_EXPIRES_AT, session.accessExpiresAt);
  localStorage.setItem(STORAGE_KEY_REFRESH_TOKEN, session.refreshToken);
  localStorage.setItem(
    STORAGE_KEY_REFRESH_EXPIRES_AT,
    session.refreshExpiresAt,
  );
  localStorage.setItem(STORAGE_KEY_USER, JSON.stringify(session.user));
}

/** Load session from localStorage. Returns null if any field is missing. */
export function loadSession(): StoredSession | null {
  const accessToken = localStorage.getItem(STORAGE_KEY_ACCESS_TOKEN);
  const accessExpiresAt = localStorage.getItem(STORAGE_KEY_ACCESS_EXPIRES_AT);
  const refreshToken = localStorage.getItem(STORAGE_KEY_REFRESH_TOKEN);
  const refreshExpiresAt = localStorage.getItem(STORAGE_KEY_REFRESH_EXPIRES_AT);
  const userJson = localStorage.getItem(STORAGE_KEY_USER);

  if (
    !accessToken ||
    !accessExpiresAt ||
    !refreshToken ||
    !refreshExpiresAt ||
    !userJson
  ) {
    return null;
  }

  try {
    const user = JSON.parse(userJson) as StoredSession["user"];
    if (!user.id || !user.email || !user.display_name) {
      return null;
    }
    return {
      accessToken,
      accessExpiresAt,
      refreshToken,
      refreshExpiresAt,
      user,
    };
  } catch {
    return null;
  }
}

/** Clear all session data from localStorage. */
export function clearSession(): void {
  localStorage.removeItem(STORAGE_KEY_ACCESS_TOKEN);
  localStorage.removeItem(STORAGE_KEY_ACCESS_EXPIRES_AT);
  localStorage.removeItem(STORAGE_KEY_REFRESH_TOKEN);
  localStorage.removeItem(STORAGE_KEY_REFRESH_EXPIRES_AT);
  localStorage.removeItem(STORAGE_KEY_USER);
}

/** Save PKCE flow data for callback retrieval. */
export function saveOAuthFlow(flow: StoredOAuthFlow): void {
  localStorage.setItem(STORAGE_KEY_OAUTH_STATE, flow.state);
  localStorage.setItem(STORAGE_KEY_CODE_VERIFIER, flow.codeVerifier);
  localStorage.setItem(STORAGE_KEY_FLOW_ID, flow.flowId);
}

/** Load PKCE flow data. Returns null if missing. */
export function loadOAuthFlow(): StoredOAuthFlow | null {
  const state = localStorage.getItem(STORAGE_KEY_OAUTH_STATE);
  const codeVerifier = localStorage.getItem(STORAGE_KEY_CODE_VERIFIER);
  const flowId = localStorage.getItem(STORAGE_KEY_FLOW_ID);
  if (!state || !codeVerifier || !flowId) {
    return null;
  }
  return { state, codeVerifier, flowId };
}

/** Clear PKCE flow data after callback. */
export function clearOAuthFlow(): void {
  localStorage.removeItem(STORAGE_KEY_OAUTH_STATE);
  localStorage.removeItem(STORAGE_KEY_CODE_VERIFIER);
  localStorage.removeItem(STORAGE_KEY_FLOW_ID);
}
