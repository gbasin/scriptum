import { create, type StoreApi, type UseBoundStore } from "zustand";

/** Connection status for the sync layer. */
export type SyncStatus = "online" | "offline" | "reconnecting";

interface SyncSnapshot {
  status: SyncStatus;
  /** ISO timestamp of last successful sync with relay. */
  lastSyncedAt: string | null;
  /** Number of pending local changes not yet confirmed by relay. */
  pendingChanges: number;
  /** Human-readable error message, if any. */
  error: string | null;
  /** Current reconnection attempt number (0 when connected). */
  reconnectAttempt: number;
}

export interface SyncStoreState extends SyncSnapshot {
  /** Transition to online status (clears error and reconnect count). */
  setOnline: () => void;
  /** Transition to offline status with optional error message. */
  setOffline: (error?: string) => void;
  /** Transition to reconnecting status, incrementing attempt counter. */
  setReconnecting: () => void;
  /** Update the last-synced timestamp (usually on relay ack). */
  setLastSyncedAt: (timestamp: string) => void;
  /** Update pending change count. */
  setPendingChanges: (count: number) => void;
  /** Full reset to initial offline state. */
  reset: () => void;
}

export type SyncStore = UseBoundStore<StoreApi<SyncStoreState>>;

const INITIAL_SNAPSHOT: SyncSnapshot = {
  status: "offline",
  lastSyncedAt: null,
  pendingChanges: 0,
  error: null,
  reconnectAttempt: 0,
};

export function createSyncStore(
  initial: Partial<SyncSnapshot> = {}
): SyncStore {
  const initialState: SyncSnapshot = { ...INITIAL_SNAPSHOT, ...initial };

  return create<SyncStoreState>()((set, get) => ({
    ...initialState,
    setOnline: () => {
      set({
        status: "online",
        error: null,
        reconnectAttempt: 0,
      });
    },
    setOffline: (error?: string) => {
      set({
        status: "offline",
        error: error ?? null,
        reconnectAttempt: 0,
      });
    },
    setReconnecting: () => {
      const previous = get();
      set({
        status: "reconnecting",
        reconnectAttempt: previous.reconnectAttempt + 1,
      });
    },
    setLastSyncedAt: (timestamp) => {
      set({ lastSyncedAt: timestamp });
    },
    setPendingChanges: (count) => {
      set({ pendingChanges: count });
    },
    reset: () => {
      set({ ...INITIAL_SNAPSHOT });
    },
  }));
}

export const useSyncStore = createSyncStore();
