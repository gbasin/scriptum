import { useCallback, useEffect, useMemo } from "react";
import type {
  AuthService,
  AuthSession,
  StartGitHubOAuthOptions,
} from "../lib/auth";
import {
  getStoredSession as getStoredSessionFromAuth,
  handleOAuthCallback as handleOAuthCallbackFromAuth,
  logout as logoutFromAuth,
  refreshAccessToken as refreshAccessTokenFromAuth,
  startGitHubOAuth as startGitHubOAuthFromAuth,
} from "../lib/auth";
import { isTauri, performTauriLogin } from "../lib/tauri-auth";
import { type AuthStatus, type AuthStore, useAuthStore } from "../store/auth";
import { type LocalIdentity, useRuntimeStore } from "../store/runtime";

const DEFAULT_REFRESH_BUFFER_MS = 60_000;
const DEFAULT_LOGIN_PATH = "/";
const DEFAULT_LOCATION_ORIGIN = "http://localhost";
const LOCAL_SESSION_TTL_MS = 10 * 365 * 24 * 60 * 60 * 1_000;

type UseAuthLocation = Pick<Location, "assign" | "origin">;

export interface UseAuthOptions {
  auth?: Pick<
    AuthService,
    | "getStoredSession"
    | "startGitHubOAuth"
    | "handleOAuthCallback"
    | "refreshAccessToken"
    | "logout"
  >;
  store?: AuthStore;
  refreshBufferMs?: number;
  loginPath?: string;
  location?: UseAuthLocation;
}

export interface UseAuthResult {
  status: AuthStatus;
  user: AuthSession["user"] | null;
  accessToken: string | null;
  error: string | null;
  isAuthenticated: boolean;
  login: (options?: StartGitHubOAuthOptions) => Promise<void>;
  logout: () => Promise<void>;
}

function setAuthenticated(store: AuthStore, session: AuthSession): void {
  store.setState({
    status: "authenticated",
    user: session.user,
    accessToken: session.accessToken,
    accessExpiresAt: session.accessExpiresAt,
    refreshToken: session.refreshToken,
    refreshExpiresAt: session.refreshExpiresAt,
    error: null,
  });
}

function setUnauthenticated(
  store: AuthStore,
  error: string | null = null,
): void {
  store.setState({
    status: "unauthenticated",
    user: null,
    accessToken: null,
    accessExpiresAt: null,
    refreshToken: null,
    refreshExpiresAt: null,
    error,
  });
}

function localSessionFromIdentity(identity: LocalIdentity): AuthSession {
  const expiresAt = new Date(Date.now() + LOCAL_SESSION_TTL_MS).toISOString();
  return {
    accessToken: `local-access-token:${identity.id}`,
    accessExpiresAt: expiresAt,
    refreshToken: `local-refresh-token:${identity.id}`,
    refreshExpiresAt: expiresAt,
    user: {
      display_name: identity.displayName,
      email: identity.email,
      id: identity.id,
    },
  };
}

function resolveLoginUrl(location: UseAuthLocation, loginPath: string): string {
  if (/^https?:\/\//.test(loginPath)) {
    return loginPath;
  }
  return new URL(loginPath, location.origin).toString();
}

function resolveLocationRef(
  location: UseAuthOptions["location"],
): UseAuthLocation {
  if (location) {
    return location;
  }

  const globalLocation = globalThis.location;
  if (
    globalLocation &&
    typeof globalLocation.origin === "string" &&
    typeof globalLocation.assign === "function"
  ) {
    return globalLocation;
  }

  return {
    assign: () => {},
    origin: DEFAULT_LOCATION_ORIGIN,
  };
}

const defaultAuth = {
  getStoredSession: getStoredSessionFromAuth,
  startGitHubOAuth: startGitHubOAuthFromAuth,
  handleOAuthCallback: handleOAuthCallbackFromAuth,
  refreshAccessToken: refreshAccessTokenFromAuth,
  logout: logoutFromAuth,
};

export function useAuth(options: UseAuthOptions = {}): UseAuthResult {
  const auth = options.auth ?? defaultAuth;
  const store = options.store ?? useAuthStore;
  const mode = useRuntimeStore((state) => state.mode);
  const localIdentity = useRuntimeStore((state) => state.localIdentity);
  const refreshBufferMs = options.refreshBufferMs ?? DEFAULT_REFRESH_BUFFER_MS;
  const loginPath = options.loginPath ?? DEFAULT_LOGIN_PATH;
  const locationRef = resolveLocationRef(options.location);
  const localSession = useMemo(
    () => localSessionFromIdentity(localIdentity),
    [localIdentity],
  );

  const status = store((state) => state.status);
  const user = store((state) => state.user);
  const accessToken = store((state) => state.accessToken);
  const accessExpiresAt = store((state) => state.accessExpiresAt);
  const error = store((state) => state.error);

  const redirectToLogin = useCallback(() => {
    locationRef.assign(resolveLoginUrl(locationRef, loginPath));
  }, [locationRef, loginPath]);

  useEffect(() => {
    if (mode === "local") {
      setAuthenticated(store, localSession);
      return;
    }

    const session = auth.getStoredSession();
    if (session) {
      setAuthenticated(store, session);
      return;
    }
    setUnauthenticated(store);
  }, [auth, localSession, mode, store]);

  useEffect(() => {
    if (mode === "local" || status !== "authenticated" || !accessExpiresAt) {
      return;
    }

    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const runRefresh = async () => {
      const refreshed = await auth.refreshAccessToken();
      if (disposed) {
        return;
      }
      if (!refreshed) {
        setUnauthenticated(store, "Session expired. Please log in again.");
        redirectToLogin();
        return;
      }
      setAuthenticated(store, refreshed);
    };

    const accessExpiresAtMs = Date.parse(accessExpiresAt);
    const delayMs = Number.isNaN(accessExpiresAtMs)
      ? 0
      : Math.max(0, accessExpiresAtMs - Date.now() - refreshBufferMs);

    timer = setTimeout(() => {
      void runRefresh();
    }, delayMs);

    return () => {
      disposed = true;
      if (timer !== null) {
        clearTimeout(timer);
      }
    };
  }, [
    accessExpiresAt,
    auth,
    mode,
    redirectToLogin,
    refreshBufferMs,
    status,
    store,
  ]);

  const login = useCallback(
    async (startOptions: StartGitHubOAuthOptions = {}) => {
      if (mode === "local") {
        setAuthenticated(store, localSession);
        return;
      }

      try {
        if (isTauri()) {
          const session = await performTauriLogin(auth);
          setAuthenticated(store, session);
          return;
        }

        const redirectUri =
          startOptions.redirectUri ??
          new URL("/auth-callback", locationRef.origin).toString();
        await auth.startGitHubOAuth({
          ...startOptions,
          redirectUri,
        });
      } catch {
        store.setState({
          error: "Login failed",
        });
      }
    },
    [auth, localSession, locationRef.origin, mode, store],
  );

  const logout = useCallback(async () => {
    if (mode === "local") {
      setAuthenticated(store, localSession);
      return;
    }

    try {
      await auth.logout();
    } finally {
      setUnauthenticated(store);
    }
  }, [auth, localSession, mode, store]);

  return {
    status,
    user,
    accessToken,
    error,
    isAuthenticated: status === "authenticated",
    login,
    logout,
  };
}
