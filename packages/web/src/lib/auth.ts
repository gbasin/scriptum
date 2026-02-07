// Web auth utilities: OAuth flow, session persistence, token refresh, logout.

import {
  AuthClient,
  type OAuthCallbackResult,
  type OAuthStartResult,
  type RefreshResult,
} from "../auth/client";
import { generateCodeChallenge, generateCodeVerifier } from "../auth/pkce";
import {
  clearSession as clearStoredSessionData,
  loadSession as loadStoredSessionData,
  type StoredSession,
  saveSession as saveStoredSessionData,
} from "../auth/storage";

const DEFAULT_RELAY_URL =
  import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";
export const DEFAULT_ACCESS_TOKEN_BUFFER_MS = 60_000;

const OAUTH_STATE_KEY = "scriptum:oauth_state";
const OAUTH_CODE_VERIFIER_KEY = "scriptum:code_verifier";
const OAUTH_FLOW_ID_KEY = "scriptum:flow_id";

interface StoredOAuthFlow {
  state: string;
  codeVerifier: string;
  flowId: string;
}

export type AuthSession = StoredSession;

export interface AuthLocation {
  assign: (url: string) => void;
  origin: string;
  search?: string;
}

export interface AuthApi {
  startOAuth(params: {
    redirect_uri: string;
    state: string;
    code_challenge: string;
    code_challenge_method: string;
  }): Promise<OAuthStartResult>;
  exchangeCode(params: {
    flow_id: string;
    code: string;
    state: string;
    code_verifier: string;
  }): Promise<OAuthCallbackResult>;
  refreshToken(refreshToken: string): Promise<RefreshResult>;
  logout(refreshToken: string): Promise<void>;
}

export interface AuthServiceOptions {
  baseUrl?: string;
  client?: AuthApi;
  localStorage?: Storage;
  sessionStorage?: Storage;
  location?: AuthLocation;
  now?: () => number;
  accessTokenBufferMs?: number;
}

export interface StartGitHubOAuthOptions {
  redirectUri?: string;
  redirect?: boolean;
  state?: string;
}

export interface StartGitHubOAuthResult {
  flowId: string;
  authorizationUrl: string;
  expiresAt: string;
  state: string;
}

export interface HandleOAuthCallbackOptions {
  searchParams?: URLSearchParams;
}

type AuthFlowErrorCode =
  | "OAUTH_CALLBACK_PARAMS_MISSING"
  | "OAUTH_FLOW_MISSING"
  | "OAUTH_STATE_MISMATCH";

export class AuthFlowError extends Error {
  constructor(
    public readonly code: AuthFlowErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "AuthFlowError";
  }
}

export interface AuthService {
  startGitHubOAuth: (
    options?: StartGitHubOAuthOptions,
  ) => Promise<StartGitHubOAuthResult>;
  handleOAuthCallback: (
    options?: HandleOAuthCallbackOptions,
  ) => Promise<AuthSession>;
  getStoredSession: () => AuthSession | null;
  refreshAccessToken: () => Promise<AuthSession | null>;
  getAccessToken: () => Promise<string | null>;
  logout: () => Promise<void>;
  clearStoredSession: () => void;
}

function parseTimestamp(iso: string): number | null {
  const ms = Date.parse(iso);
  return Number.isNaN(ms) ? null : ms;
}

export function createAuthService(
  options: AuthServiceOptions = {},
): AuthService {
  const localStorageRef = options.localStorage ?? globalThis.localStorage;
  const sessionStorageRef = options.sessionStorage ?? globalThis.sessionStorage;
  const locationRef = options.location ?? globalThis.location;
  const now = options.now ?? (() => Date.now());
  const accessTokenBufferMs =
    options.accessTokenBufferMs ?? DEFAULT_ACCESS_TOKEN_BUFFER_MS;

  const client =
    options.client ??
    new AuthClient({ baseUrl: options.baseUrl ?? DEFAULT_RELAY_URL });

  let refreshPromise: Promise<AuthSession | null> | null = null;

  const saveOAuthFlow = (flow: StoredOAuthFlow): void => {
    sessionStorageRef.setItem(OAUTH_STATE_KEY, flow.state);
    sessionStorageRef.setItem(OAUTH_CODE_VERIFIER_KEY, flow.codeVerifier);
    sessionStorageRef.setItem(OAUTH_FLOW_ID_KEY, flow.flowId);
  };

  const loadOAuthFlow = (): StoredOAuthFlow | null => {
    const state = sessionStorageRef.getItem(OAUTH_STATE_KEY);
    const codeVerifier = sessionStorageRef.getItem(OAUTH_CODE_VERIFIER_KEY);
    const flowId = sessionStorageRef.getItem(OAUTH_FLOW_ID_KEY);

    if (!state || !codeVerifier || !flowId) {
      return null;
    }
    return { state, codeVerifier, flowId };
  };

  const clearOAuthFlow = (): void => {
    sessionStorageRef.removeItem(OAUTH_STATE_KEY);
    sessionStorageRef.removeItem(OAUTH_CODE_VERIFIER_KEY);
    sessionStorageRef.removeItem(OAUTH_FLOW_ID_KEY);
  };

  const clearStoredSession = (): void => {
    clearStoredSessionData();
    clearOAuthFlow();
  };

  const isExpired = (iso: string, bufferMs = 0): boolean => {
    const timestamp = parseTimestamp(iso);
    if (timestamp === null) {
      return true;
    }
    return timestamp - bufferMs <= now();
  };

  const getStoredSession = (): AuthSession | null => {
    const session = loadStoredSessionData();
    if (!session) {
      return null;
    }
    if (isExpired(session.refreshExpiresAt, 0)) {
      clearStoredSession();
      return null;
    }
    return session;
  };

  const refreshAccessToken = async (): Promise<AuthSession | null> => {
    if (refreshPromise) {
      return refreshPromise;
    }

    const current = getStoredSession();
    if (!current) {
      return null;
    }

    refreshPromise = (async () => {
      try {
        const refreshed = await client.refreshToken(current.refreshToken);
        const nextSession: AuthSession = {
          accessToken: refreshed.access_token,
          accessExpiresAt: refreshed.access_expires_at,
          refreshToken: refreshed.refresh_token,
          refreshExpiresAt: refreshed.refresh_expires_at,
          user: current.user,
        };
        saveStoredSessionData(nextSession);
        return nextSession;
      } catch {
        clearStoredSession();
        return null;
      }
    })();

    try {
      return await refreshPromise;
    } finally {
      refreshPromise = null;
    }
  };

  const getAccessToken = async (): Promise<string | null> => {
    const session = getStoredSession();
    if (!session) {
      return null;
    }

    if (!isExpired(session.accessExpiresAt, accessTokenBufferMs)) {
      return session.accessToken;
    }

    const refreshed = await refreshAccessToken();
    return refreshed?.accessToken ?? null;
  };

  const startGitHubOAuth = async (
    startOptions: StartGitHubOAuthOptions = {},
  ): Promise<StartGitHubOAuthResult> => {
    const codeVerifier = generateCodeVerifier();
    const codeChallenge = await generateCodeChallenge(codeVerifier);
    const state = startOptions.state ?? crypto.randomUUID();

    const redirectUri =
      startOptions.redirectUri ??
      new URL("/auth-callback", locationRef.origin).toString();

    const startResult = await client.startOAuth({
      redirect_uri: redirectUri,
      state,
      code_challenge: codeChallenge,
      code_challenge_method: "S256",
    });

    saveOAuthFlow({
      state,
      codeVerifier,
      flowId: startResult.flow_id,
    });

    if (startOptions.redirect !== false) {
      locationRef.assign(startResult.authorization_url);
    }

    return {
      flowId: startResult.flow_id,
      authorizationUrl: startResult.authorization_url,
      expiresAt: startResult.expires_at,
      state,
    };
  };

  const handleOAuthCallback = async (
    callbackOptions: HandleOAuthCallbackOptions = {},
  ): Promise<AuthSession> => {
    const params =
      callbackOptions.searchParams ??
      new URLSearchParams(locationRef.search ?? "");

    const code = params.get("code");
    const state = params.get("state");

    if (!code || !state) {
      throw new AuthFlowError(
        "OAUTH_CALLBACK_PARAMS_MISSING",
        "OAuth callback is missing code or state parameter.",
      );
    }

    const flow = loadOAuthFlow();
    if (!flow) {
      throw new AuthFlowError(
        "OAUTH_FLOW_MISSING",
        "OAuth flow data not found. Start login again.",
      );
    }

    if (flow.state !== state) {
      clearOAuthFlow();
      throw new AuthFlowError(
        "OAUTH_STATE_MISMATCH",
        "OAuth state mismatch. Start login again.",
      );
    }

    try {
      const callbackResult = await client.exchangeCode({
        flow_id: flow.flowId,
        code,
        state,
        code_verifier: flow.codeVerifier,
      });

      const session: AuthSession = {
        accessToken: callbackResult.access_token,
        accessExpiresAt: callbackResult.access_expires_at,
        refreshToken: callbackResult.refresh_token,
        refreshExpiresAt: callbackResult.refresh_expires_at,
        user: callbackResult.user,
      };

      saveStoredSessionData(session);
      return session;
    } finally {
      clearOAuthFlow();
    }
  };

  const logout = async (): Promise<void> => {
    const current = getStoredSession();
    if (current?.refreshToken) {
      try {
        await client.logout(current.refreshToken);
      } catch {
        // Best-effort server logout.
      }
    }
    clearStoredSession();
  };

  return {
    startGitHubOAuth,
    handleOAuthCallback,
    getStoredSession,
    refreshAccessToken,
    getAccessToken,
    logout,
    clearStoredSession,
  };
}

let defaultAuthService: AuthService | null = null;

function getDefaultAuthService(): AuthService {
  if (!defaultAuthService) {
    defaultAuthService = createAuthService();
  }
  return defaultAuthService;
}

export async function startGitHubOAuth(
  options?: StartGitHubOAuthOptions,
): Promise<StartGitHubOAuthResult> {
  return getDefaultAuthService().startGitHubOAuth(options);
}

export async function handleOAuthCallback(
  options?: HandleOAuthCallbackOptions,
): Promise<AuthSession> {
  return getDefaultAuthService().handleOAuthCallback(options);
}

export function getStoredSession(): AuthSession | null {
  return getDefaultAuthService().getStoredSession();
}

export async function refreshAccessToken(): Promise<AuthSession | null> {
  return getDefaultAuthService().refreshAccessToken();
}

export async function getAccessToken(): Promise<string | null> {
  return getDefaultAuthService().getAccessToken();
}

export async function logout(): Promise<void> {
  return getDefaultAuthService().logout();
}

export function clearStoredSession(): void {
  return getDefaultAuthService().clearStoredSession();
}

export function resetAuthServiceForTests(): void {
  defaultAuthService = null;
}
