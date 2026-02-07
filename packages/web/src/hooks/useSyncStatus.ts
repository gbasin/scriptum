import type { CollaborationProvider } from "@scriptum/editor";
import { useEffect } from "react";
import {
  type ReconnectProgress,
  type SyncStatus,
  type SyncStore,
  useSyncStore,
} from "../store/sync";

export type DerivedSyncStatus =
  | "synced"
  | "syncing"
  | "offline"
  | "reconnecting"
  | "error";

export interface UseSyncStatusOptions {
  provider: CollaborationProvider | null;
  pendingChangesCount?: number;
  reconnectProgress?: ReconnectProgress | null;
  store?: SyncStore;
}

export interface UseSyncStatusResult {
  status: DerivedSyncStatus;
  pendingChangesCount: number;
  lastSyncedAt: string | null;
  reconnectProgress: ReconnectProgress | null;
  error: string | null;
}

function normalizePendingChangesCount(value: number | undefined): number | undefined {
  if (value === undefined || Number.isNaN(value)) {
    return undefined;
  }
  return Math.max(0, Math.floor(value));
}

function deriveSyncStatus(
  rawStatus: SyncStatus,
  pendingChangesCount: number,
  reconnectProgress: ReconnectProgress | null,
  error: string | null,
): DerivedSyncStatus {
  if (error !== null) {
    return "error";
  }
  if (rawStatus === "offline") {
    return "offline";
  }
  if (rawStatus === "reconnecting") {
    return "reconnecting";
  }
  if (
    reconnectProgress !== null &&
    reconnectProgress.totalUpdates > 0 &&
    reconnectProgress.syncedUpdates < reconnectProgress.totalUpdates
  ) {
    return "syncing";
  }
  if (pendingChangesCount > 0) {
    return "syncing";
  }
  return "synced";
}

export function useSyncStatus(options: UseSyncStatusOptions): UseSyncStatusResult {
  const store = options.store ?? useSyncStore;
  const provider = options.provider;
  const pendingChangesCountInput = normalizePendingChangesCount(options.pendingChangesCount);

  const storeStatus = store((state) => state.status);
  const lastSyncedAt = store((state) => state.lastSyncedAt);
  const pendingChangesCount = store((state) => state.pendingChanges);
  const reconnectProgress = store((state) => state.reconnectProgress);
  const error = store((state) => state.error);

  useEffect(() => {
    if (pendingChangesCountInput === undefined) {
      return;
    }
    const state = store.getState();
    if (state.pendingChanges === pendingChangesCountInput) {
      return;
    }
    state.setPendingChanges(pendingChangesCountInput);
  }, [pendingChangesCountInput, store]);

  useEffect(() => {
    if (options.reconnectProgress === undefined) {
      return;
    }
    store.getState().setReconnectProgress(options.reconnectProgress);
  }, [options.reconnectProgress, store]);

  useEffect(() => {
    if (!provider) {
      const state = store.getState();
      if (state.status !== "offline" || state.error === null) {
        state.setOffline(state.error ?? undefined);
      }
      return;
    }

    provider.provider.on("status", ({ status }) => {
      const state = store.getState();
      if (status === "connected") {
        state.setOnline();
        state.setLastSyncedAt(new Date().toISOString());
        return;
      }
      if (typeof navigator !== "undefined" && navigator.onLine === false) {
        state.setOffline("network offline");
        return;
      }
      state.setReconnecting();
    });

    return () => {
      store.getState().setOffline();
    };
  }, [provider, store]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const handleOffline = () => {
      store.getState().setOffline("network offline");
    };
    const handleOnline = () => {
      if (provider) {
        store.getState().setReconnecting();
      }
    };

    window.addEventListener("offline", handleOffline);
    window.addEventListener("online", handleOnline);
    return () => {
      window.removeEventListener("offline", handleOffline);
      window.removeEventListener("online", handleOnline);
    };
  }, [provider, store]);

  return {
    status: deriveSyncStatus(storeStatus, pendingChangesCount, reconnectProgress, error),
    pendingChangesCount,
    lastSyncedAt,
    reconnectProgress,
    error,
  };
}
