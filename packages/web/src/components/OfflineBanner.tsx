import type { DerivedSyncStatus } from "../hooks/useSyncStatus";
import type { ReconnectProgress } from "../store/sync";
import { useEffect, useState } from "react";

const DEFAULT_REAPPEAR_DELAY_MS = 30_000;
const NUMBER_FORMAT = new Intl.NumberFormat("en-US");

export interface OfflineBannerProps {
  status: DerivedSyncStatus;
  reconnectProgress?: ReconnectProgress | null;
  reappearAfterMs?: number;
}

function formatUpdates(value: number): string {
  return NUMBER_FORMAT.format(Math.max(0, Math.floor(value)));
}

function shouldShowReconnectProgress(
  status: DerivedSyncStatus,
  reconnectProgress: ReconnectProgress | null,
): boolean {
  if (status !== "reconnecting" && status !== "syncing") {
    return false;
  }
  if (!reconnectProgress) {
    return false;
  }
  return reconnectProgress.totalUpdates > 0;
}

export function OfflineBanner({
  status,
  reconnectProgress = null,
  reappearAfterMs = DEFAULT_REAPPEAR_DELAY_MS,
}: OfflineBannerProps) {
  const [dismissed, setDismissed] = useState(false);
  const showOffline = status === "offline" && !dismissed;
  const showReconnectProgress = shouldShowReconnectProgress(status, reconnectProgress);
  const visible = showOffline || showReconnectProgress;

  useEffect(() => {
    if (status !== "offline") {
      setDismissed(false);
      return;
    }
    if (!dismissed) {
      return;
    }
    const timeout = window.setTimeout(() => {
      setDismissed(false);
    }, reappearAfterMs);
    return () => {
      window.clearTimeout(timeout);
    };
  }, [dismissed, reappearAfterMs, status]);

  const message = showOffline
    ? "You are offline — changes will sync when reconnected."
    : showReconnectProgress && reconnectProgress
      ? `Syncing... ${formatUpdates(reconnectProgress.syncedUpdates)}/${formatUpdates(
          reconnectProgress.totalUpdates,
        )} updates`
      : "";

  return (
    <aside
      aria-hidden={!visible}
      aria-label="Offline banner"
      data-testid="offline-banner"
      role="status"
      style={{
        background: "#fef3c7",
        border: visible ? "1px solid #f59e0b" : "0 solid transparent",
        borderRadius: "0.5rem",
        color: "#78350f",
        marginBottom: visible ? "0.75rem" : "0",
        maxHeight: visible ? "4rem" : "0",
        opacity: visible ? 1 : 0,
        overflow: "hidden",
        padding: visible ? "0.625rem 0.75rem" : "0 0.75rem",
        transform: visible ? "translateY(0)" : "translateY(-0.25rem)",
        transition:
          "opacity 180ms ease, transform 180ms ease, max-height 180ms ease, margin 180ms ease, padding 180ms ease, border-width 180ms ease",
      }}
    >
      {visible ? (
        <div
          style={{
            alignItems: "center",
            display: "flex",
            gap: "0.5rem",
            justifyContent: "space-between",
          }}
        >
          <span data-testid="offline-banner-message">{message}</span>
          {showOffline ? (
            <button
              aria-label="Dismiss offline banner"
              data-testid="offline-banner-dismiss"
              onClick={() => setDismissed(true)}
              style={{
                background: "transparent",
                border: "1px solid #b45309",
                borderRadius: "9999px",
                color: "#78350f",
                cursor: "pointer",
                fontSize: "0.75rem",
                height: "1.25rem",
                lineHeight: "1rem",
                minWidth: "1.25rem",
                padding: 0,
              }}
              type="button"
            >
              x
            </button>
          ) : null}
        </div>
      ) : null}
    </aside>
  );
}
