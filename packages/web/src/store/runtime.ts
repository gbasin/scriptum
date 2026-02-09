import { DAEMON_WHOAMI_URL } from "@scriptum/shared";
import { create } from "zustand";

export type RuntimeMode = "relay" | "local";

export interface LocalIdentity {
  id: string;
  displayName: string;
  email: string;
}

interface RuntimeStoreState {
  mode: RuntimeMode;
  modeResolved: boolean;
  localIdentity: LocalIdentity;
  setMode: (mode: RuntimeMode) => void;
  setModeResolved: (resolved: boolean) => void;
  setLocalIdentity: (identity: LocalIdentity) => void;
  reset: () => void;
}

const RELAY_REACHABILITY_TIMEOUT_MS = 2_000;
const LOCAL_IDENTITY_FALLBACK_NAME = "Local User";
const DEFAULT_RELAY_URL =
  (import.meta.env.VITE_SCRIPTUM_RELAY_URL as string | undefined) ??
  "http://localhost:8080";
const DEFAULT_DAEMON_WHOAMI_URL =
  (import.meta.env.VITE_SCRIPTUM_DAEMON_WHOAMI_URL as string | undefined) ??
  DAEMON_WHOAMI_URL;

export const DEFAULT_LOCAL_IDENTITY: LocalIdentity = {
  id: "local-user",
  displayName: LOCAL_IDENTITY_FALLBACK_NAME,
  email: "local-user@scriptum.local",
};

const INITIAL_STATE: Pick<
  RuntimeStoreState,
  "mode" | "modeResolved" | "localIdentity"
> = {
  mode: "relay",
  modeResolved: false,
  localIdentity: DEFAULT_LOCAL_IDENTITY,
};

export const useRuntimeStore = create<RuntimeStoreState>()((set) => ({
  ...INITIAL_STATE,
  setMode: (mode) => set({ mode }),
  setModeResolved: (modeResolved) => set({ modeResolved }),
  setLocalIdentity: (localIdentity) => set({ localIdentity }),
  reset: () => set({ ...INITIAL_STATE }),
}));

function readStringRecordValue(
  record: Record<string, unknown>,
  key: string,
): string | null {
  const value = record[key];
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function slugifyLocalIdentity(value: string): string {
  const normalized = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || "local-user";
}

function toLocalIdentity(name: string): LocalIdentity {
  const displayName =
    name.trim().length > 0 ? name.trim() : LOCAL_IDENTITY_FALLBACK_NAME;
  const slug = slugifyLocalIdentity(displayName);
  return {
    id: `local-${slug}`,
    displayName,
    email: `${slug}@scriptum.local`,
  };
}

function parseBooleanEnv(value: string | undefined): boolean {
  if (!value) {
    return false;
  }
  const normalized = value.trim().toLowerCase();
  return normalized === "1" || normalized === "true" || normalized === "yes";
}

async function fetchWithTimeout(
  url: string,
  timeoutMs: number,
): Promise<Response> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, {
      cache: "no-store",
      method: "HEAD",
      mode: "no-cors",
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timeout);
  }
}

async function isRelayReachable(): Promise<boolean> {
  try {
    await fetchWithTimeout(DEFAULT_RELAY_URL, RELAY_REACHABILITY_TIMEOUT_MS);
    return true;
  } catch {
    return false;
  }
}

async function identityFromDaemonWhoami(): Promise<string | null> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 1_000);

  try {
    const response = await fetch(DEFAULT_DAEMON_WHOAMI_URL, {
      cache: "no-store",
      method: "GET",
      signal: controller.signal,
    });
    if (!response.ok) {
      return null;
    }
    const payload = (await response.json()) as unknown;
    if (!payload || typeof payload !== "object") {
      return null;
    }
    const record = payload as Record<string, unknown>;
    return (
      readStringRecordValue(record, "display_name") ??
      readStringRecordValue(record, "name") ??
      readStringRecordValue(record, "agent_id")
    );
  } catch {
    return null;
  } finally {
    clearTimeout(timeout);
  }
}

function identityFromEnvironment(): string | null {
  const explicit =
    (import.meta.env.VITE_SCRIPTUM_LOCAL_USER_NAME as string | undefined) ??
    (import.meta.env.VITE_SCRIPTUM_LOCAL_USER as string | undefined);
  if (explicit && explicit.trim().length > 0) {
    return explicit.trim();
  }

  const processEnv = (
    globalThis as {
      process?: { env?: Record<string, string | undefined> };
    }
  ).process?.env;
  const processUser = processEnv?.USER ?? processEnv?.USERNAME;
  if (processUser && processUser.trim().length > 0) {
    return processUser.trim();
  }

  return null;
}

async function resolveLocalIdentity(): Promise<LocalIdentity> {
  const fromEnv = identityFromEnvironment();
  if (fromEnv) {
    return toLocalIdentity(fromEnv);
  }

  const fromWhoami = await identityFromDaemonWhoami();
  if (fromWhoami) {
    return toLocalIdentity(fromWhoami);
  }

  return DEFAULT_LOCAL_IDENTITY;
}

async function resolveRuntimeMode(): Promise<RuntimeMode> {
  if (
    parseBooleanEnv(
      import.meta.env.VITE_SCRIPTUM_LOCAL_MODE as string | undefined,
    )
  ) {
    return "local";
  }

  return (await isRelayReachable()) ? "relay" : "local";
}

export async function initializeRuntimeMode(): Promise<RuntimeMode> {
  const mode = await resolveRuntimeMode();
  const nextState: Partial<RuntimeStoreState> = {
    mode,
    modeResolved: true,
  };

  if (mode === "local") {
    nextState.localIdentity = await resolveLocalIdentity();
  }

  useRuntimeStore.setState(nextState);
  return mode;
}
