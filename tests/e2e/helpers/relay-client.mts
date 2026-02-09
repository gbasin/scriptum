// Direct HTTP client for relay password auth — bypasses the web frontend.

const RELAY_URL =
  process.env.VITE_SCRIPTUM_RELAY_URL ?? "http://127.0.0.1:8080";

interface RegisterParams {
  email: string;
  display_name: string;
  password: string;
}

interface LoginParams {
  email: string;
  password: string;
}

interface OAuthStartParams {
  redirect_uri: string;
  state: string;
  code_challenge: string;
  code_challenge_method: string;
}

interface TokenResponse {
  access_token: string;
  access_expires_at: string;
  refresh_token: string;
  refresh_expires_at: string;
  user?: { id: string; email: string; display_name: string };
}

interface OAuthStartResponse {
  flow_id: string;
  authorization_url: string;
  expires_at: string;
}

async function relayFetch(
  path: string,
  options: RequestInit = {},
): Promise<Response> {
  return fetch(`${RELAY_URL}${path}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...options.headers,
    },
  });
}

/** Check if the relay is reachable. */
export async function isRelayAvailable(): Promise<boolean> {
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 2_000);
    const res = await fetch(`${RELAY_URL}/healthz`, {
      signal: controller.signal,
    });
    clearTimeout(timeout);
    return res.ok;
  } catch {
    return false;
  }
}

/** Register a new user via password auth. */
export async function registerUser(
  params: RegisterParams,
): Promise<TokenResponse> {
  const res = await relayFetch("/v1/auth/password/register", {
    method: "POST",
    body: JSON.stringify(params),
  });
  if (!res.ok) {
    throw new Error(`Register failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<TokenResponse>;
}

/** Login an existing user via password auth. */
export async function loginUser(params: LoginParams): Promise<TokenResponse> {
  const res = await relayFetch("/v1/auth/password/login", {
    method: "POST",
    body: JSON.stringify(params),
  });
  if (!res.ok) {
    throw new Error(`Login failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<TokenResponse>;
}

/** Refresh an access token. Returns new token pair. */
export async function refreshToken(token: string): Promise<TokenResponse> {
  const res = await relayFetch("/v1/auth/token/refresh", {
    method: "POST",
    body: JSON.stringify({ refresh_token: token }),
  });
  if (!res.ok) {
    throw new Error(`Refresh failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<TokenResponse>;
}

/** Logout — revoke the session. */
export async function logoutUser(
  accessToken: string,
  refreshTokenValue: string,
): Promise<Response> {
  return relayFetch("/v1/auth/logout", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify({ refresh_token: refreshTokenValue }),
  });
}

/** Start an OAuth flow (PKCE). */
export async function startOAuthFlow(
  params: OAuthStartParams,
): Promise<OAuthStartResponse> {
  const res = await relayFetch("/v1/auth/oauth/github/start", {
    method: "POST",
    body: JSON.stringify(params),
  });
  if (!res.ok) {
    throw new Error(`OAuth start failed: ${res.status} ${await res.text()}`);
  }
  return res.json() as Promise<OAuthStartResponse>;
}

/** Generate a collision-free email for parallel tests. */
export function uniqueEmail(prefix = "e2e"): string {
  const id = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  return `${prefix}-${id}@test.scriptum.local`;
}
