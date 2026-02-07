import type { CursorPosition, SyncState } from "../test/harness";

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
  dotColor: string;
  colorName: "green" | "yellow" | "red";
}

const SYNC_BADGE: Record<SyncState, SyncBadgeConfig> = {
  synced: { label: "Synced", dotColor: "#16a34a", colorName: "green" },
  reconnecting: {
    label: "Reconnecting",
    dotColor: "#eab308",
    colorName: "yellow",
  },
  offline: { label: "Offline", dotColor: "#dc2626", colorName: "red" },
  error: { label: "Error", dotColor: "#dc2626", colorName: "red" },
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
    <footer
      aria-label="Status bar"
      data-testid="status-bar"
      style={{
        alignItems: "center",
        borderTop: "1px solid #e5e7eb",
        color: "#1f2937",
        display: "flex",
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: "0.85rem",
        gap: "1rem",
        marginTop: "1rem",
        paddingTop: "0.5rem",
      }}
    >
      <span
        style={{ alignItems: "center", display: "inline-flex", gap: "0.5rem" }}
      >
        <span
          aria-hidden="true"
          data-sync-color={badge.colorName}
          data-testid="status-sync-dot"
          style={{
            backgroundColor: badge.dotColor,
            borderRadius: "999px",
            display: "inline-block",
            height: "0.625rem",
            width: "0.625rem",
          }}
        />
        <span aria-label="Sync state" data-testid="sync-state" role="status">
          Sync: {badge.label}
        </span>
      </span>
      <span data-testid="status-cursor">
        Ln {line}, Col {col}
      </span>
      <span data-testid="status-editor-count">Editors: {activeEditors}</span>
      {pendingUpdates > 0 ? (
        <span data-testid="status-pending-updates">
          Pending: {pendingUpdates}
        </span>
      ) : null}
      {showReconnectProgress ? (
        <span data-testid="status-reconnect-progress">
          Syncing... {syncedUpdates}/{reconnectProgress.totalUpdates} updates
        </span>
      ) : null}
    </footer>
  );
}
