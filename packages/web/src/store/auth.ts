// Auth store — session state, login/logout, token refresh.

import { create, type StoreApi, type UseBoundStore } from "zustand";
import {
  type AuthClient,
  AuthClientError,
  type AuthUser,
} from "../auth/client";
import { generateCodeChallenge, generateCodeVerifier } from "../auth/pkce";
import {
  clearOAuthFlow,
  clearSession,
  loadOAuthFlow,
  loadSession,
  saveOAuthFlow,
  saveSession,
} from "../auth/storage";

export type AuthStatus = "unknown" | "authenticated" | "unauthenticated";

interface AuthSnapshot {
  status: AuthStatus;
  user: AuthUser | null;
  accessToken: string | null;
  accessExpiresAt: string | null;
  refreshToken: string | null;
  refreshExpiresAt: string | null;
  error: string | null;
}

export interface AuthStoreState extends AuthSnapshot {
  /** Restore session from localStorage on app boot. */
  restoreSession: () => void;
  /** Start GitHub OAuth flow — generates PKCE, calls /start, redirects. */
  startLogin: (client: AuthClient, redirectUri: string) => Promise<void>;
  /** Handle OAuth callback — exchange code for tokens. */
  handleCallback: (
    client: AuthClient,
    code: string,
    state: string,
  ) => Promise<void>;
  /** Refresh the access token using the stored refresh token. */
  refreshAccessToken: (client: AuthClient) => Promise<boolean>;
  /** Log out — revoke session on server + clear local state. */
  logout: (client: AuthClient) => Promise<void>;
  /** Check if access token is expired or about to expire (within buffer). */
  isAccessTokenExpired: (bufferMs?: number) => boolean;
  /** Full reset (for tests). */
  reset: () => void;
}

export type AuthStore = UseBoundStore<StoreApi<AuthStoreState>>;

const INITIAL_SNAPSHOT: AuthSnapshot = {
  status: "unknown",
  user: null,
  accessToken: null,
  accessExpiresAt: null,
  refreshToken: null,
  refreshExpiresAt: null,
  error: null,
};

/** Default expiry buffer: refresh 60s before token expires. */
const DEFAULT_EXPIRY_BUFFER_MS = 60_000;

export function createAuthStore(
  initial: Partial<AuthSnapshot> = {},
): AuthStore {
  const initialState: AuthSnapshot = { ...INITIAL_SNAPSHOT, ...initial };

  return create<AuthStoreState>()((set, get) => ({
    ...initialState,

    restoreSession: () => {
      const session = loadSession();
      if (!session) {
        set({ ...INITIAL_SNAPSHOT, status: "unauthenticated" });
        return;
      }

      // Check if refresh token has expired.
      if (new Date(session.refreshExpiresAt).getTime() < Date.now()) {
        clearSession();
        set({ ...INITIAL_SNAPSHOT, status: "unauthenticated" });
        return;
      }

      set({
        status: "authenticated",
        user: session.user,
        accessToken: session.accessToken,
        accessExpiresAt: session.accessExpiresAt,
        refreshToken: session.refreshToken,
        refreshExpiresAt: session.refreshExpiresAt,
        error: null,
      });
    },

    startLogin: async (client, redirectUri) => {
      try {
        const codeVerifier = generateCodeVerifier();
        const codeChallenge = await generateCodeChallenge(codeVerifier);
        const state = crypto.randomUUID();

        const result = await client.startOAuth({
          redirect_uri: redirectUri,
          state,
          code_challenge: codeChallenge,
          code_challenge_method: "S256",
        });

        saveOAuthFlow({
          state,
          codeVerifier,
          flowId: result.flow_id,
        });

        // Redirect to GitHub.
        window.location.href = result.authorization_url;
      } catch (err) {
        const message =
          err instanceof AuthClientError
            ? `Login failed (${err.status})`
            : "Login failed";
        set({ error: message });
      }
    },

    handleCallback: async (client, code, state) => {
      try {
        const flow = loadOAuthFlow();
        if (!flow) {
          set({
            status: "unauthenticated",
            error: "OAuth flow data not found — please try logging in again.",
          });
          return;
        }

        if (flow.state !== state) {
          clearOAuthFlow();
          set({
            status: "unauthenticated",
            error: "OAuth state mismatch — possible CSRF. Please try again.",
          });
          return;
        }

        const result = await client.exchangeCode({
          flow_id: flow.flowId,
          code,
          state,
          code_verifier: flow.codeVerifier,
        });

        clearOAuthFlow();

        const session = {
          accessToken: result.access_token,
          accessExpiresAt: result.access_expires_at,
          refreshToken: result.refresh_token,
          refreshExpiresAt: result.refresh_expires_at,
          user: result.user,
        };
        saveSession(session);

        set({
          status: "authenticated",
          ...session,
          error: null,
        });
      } catch (err) {
        clearOAuthFlow();
        const message =
          err instanceof AuthClientError
            ? `Authentication failed (${err.status})`
            : "Authentication failed";
        set({ status: "unauthenticated", error: message });
      }
    },

    refreshAccessToken: async (client) => {
      const { refreshToken } = get();
      if (!refreshToken) {
        set({ status: "unauthenticated", error: null });
        clearSession();
        return false;
      }

      try {
        const result = await client.refreshToken(refreshToken);

        const prevState = get();
        const session = {
          accessToken: result.access_token,
          accessExpiresAt: result.access_expires_at,
          refreshToken: result.refresh_token,
          refreshExpiresAt: result.refresh_expires_at,
          user: prevState.user!,
        };
        saveSession(session);

        set({
          accessToken: result.access_token,
          accessExpiresAt: result.access_expires_at,
          refreshToken: result.refresh_token,
          refreshExpiresAt: result.refresh_expires_at,
          error: null,
        });
        return true;
      } catch {
        clearSession();
        set({
          ...INITIAL_SNAPSHOT,
          status: "unauthenticated",
          error: "Session expired. Please log in again.",
        });
        return false;
      }
    },

    logout: async (client) => {
      const { refreshToken } = get();
      if (refreshToken) {
        try {
          await client.logout(refreshToken);
        } catch {
          // Best-effort server revocation; clear local state regardless.
        }
      }
      clearSession();
      set({ ...INITIAL_SNAPSHOT, status: "unauthenticated" });
    },

    isAccessTokenExpired: (bufferMs = DEFAULT_EXPIRY_BUFFER_MS) => {
      const { accessExpiresAt } = get();
      if (!accessExpiresAt) return true;
      return new Date(accessExpiresAt).getTime() - bufferMs < Date.now();
    },

    reset: () => {
      set({ ...INITIAL_SNAPSHOT });
    },
  }));
}

export const useAuthStore = createAuthStore();
