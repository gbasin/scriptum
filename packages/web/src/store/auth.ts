// Auth store — session state, login/logout, token refresh.

import { create, type StoreApi, type UseBoundStore } from "zustand";
import {
  type AuthClient,
  AuthClientError,
  type AuthUser,
} from "../auth/client";
import {
  AuthFlowError,
  type AuthLocation,
  createAuthService,
} from "../lib/auth";

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

function getAuthLocation(): AuthLocation {
  const locationRef = globalThis.location as
    | (Partial<Location> & { href?: string })
    | undefined;

  return {
    origin: locationRef?.origin ?? "http://localhost",
    search: locationRef?.search,
    assign: (url: string) => {
      if (locationRef && typeof locationRef.assign === "function") {
        locationRef.assign(url);
        return;
      }

      if (locationRef && "href" in locationRef) {
        locationRef.href = url;
      }
    },
  };
}

function createStoreAuthService(client?: AuthClient) {
  return createAuthService({
    client,
    location: getAuthLocation(),
  });
}

export function createAuthStore(
  initial: Partial<AuthSnapshot> = {},
): AuthStore {
  const initialState: AuthSnapshot = { ...INITIAL_SNAPSHOT, ...initial };

  return create<AuthStoreState>()((set, get) => ({
    ...initialState,

    restoreSession: () => {
      const session = createStoreAuthService().getStoredSession();
      if (!session) {
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
        await createStoreAuthService(client).startGitHubOAuth({
          redirectUri,
        });
        set({ error: null });
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
        const session = await createStoreAuthService(
          client,
        ).handleOAuthCallback({
          searchParams: new URLSearchParams({ code, state }),
        });

        set({
          status: "authenticated",
          ...session,
          error: null,
        });
      } catch (err) {
        let message = "Authentication failed";

        if (err instanceof AuthFlowError) {
          if (err.code === "OAUTH_FLOW_MISSING") {
            message =
              "OAuth flow data not found — please try logging in again.";
          } else if (err.code === "OAUTH_STATE_MISMATCH") {
            message = "OAuth state mismatch — possible CSRF. Please try again.";
          }
        } else if (err instanceof AuthClientError) {
          message = `Authentication failed (${err.status})`;
        }

        set({
          ...INITIAL_SNAPSHOT,
          status: "unauthenticated",
          error: message,
        });
      }
    },

    refreshAccessToken: async (client) => {
      const session = await createStoreAuthService(client).refreshAccessToken();
      if (!session) {
        set({
          ...INITIAL_SNAPSHOT,
          status: "unauthenticated",
          error: "Session expired. Please log in again.",
        });
        return false;
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
      return true;
    },

    logout: async (client) => {
      await createStoreAuthService(client).logout();
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
