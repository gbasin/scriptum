import clsx from "clsx";
import type { CursorPosition, SyncState } from "../test/harness";
import styles from "./StatusBar.module.css";

interface StatusBarProps {
  syncState: SyncState;
  cursor: CursorPosition;
  activeEditors: number;
  pendingUpdates?: number;
  reconnectProgress?: {
    syncedUpdates: number;
    totalUpdates: number;
  } | null;
}

interface SyncBadgeConfig {
  label: string;
  colorName: "green" | "yellow" | "red";
}

const SYNC_BADGE: Record<SyncState, SyncBadgeConfig> = {
  synced: { label: "Synced", colorName: "green" },
  reconnecting: {
    label: "Reconnecting",
    colorName: "yellow",
  },
  offline: { label: "Offline", colorName: "red" },
  error: { label: "Error", colorName: "red" },
};

export function StatusBar({
  syncState,
  cursor,
  activeEditors,
  pendingUpdates = 0,
  reconnectProgress = null,
}: StatusBarProps) {
  const badge = SYNC_BADGE[syncState];
  const line = cursor.line + 1;
  const col = cursor.ch + 1;
  const showReconnectProgress =
    syncState === "reconnecting" &&
    reconnectProgress !== null &&
    reconnectProgress.totalUpdates > 0;
  const syncedUpdates = showReconnectProgress
    ? Math.min(reconnectProgress.syncedUpdates, reconnectProgress.totalUpdates)
    : 0;

  return (
    <footer className={styles.root} data-testid="status-bar">
      <span className={styles.syncCluster}>
        <span
          aria-hidden="true"
          className={clsx(
            styles.syncDot,
            badge.colorName === "green" && styles.dotGreen,
            badge.colorName === "yellow" && styles.dotYellow,
            badge.colorName === "red" && styles.dotRed,
          )}
          data-sync-color={badge.colorName}
          data-testid="status-sync-dot"
        />
        <output
          aria-label="Sync state"
          className={styles.metric}
          data-testid="sync-state"
        >
          Sync: {badge.label}
        </output>
      </span>
      <span className={styles.metric} data-testid="status-cursor">
        Ln {line}, Col {col}
      </span>
      <span className={styles.metric} data-testid="status-editor-count">
        Editors: {activeEditors}
      </span>
      {pendingUpdates > 0 ? (
        <span className={styles.metric} data-testid="status-pending-updates">
          Pending: {pendingUpdates}
        </span>
      ) : null}
      {showReconnectProgress ? (
        <span className={styles.metric} data-testid="status-reconnect-progress">
          Syncing... {syncedUpdates}/{reconnectProgress.totalUpdates} updates
        </span>
      ) : null}
    </footer>
  );
}
