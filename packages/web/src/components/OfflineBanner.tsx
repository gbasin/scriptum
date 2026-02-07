import clsx from "clsx";
import { useEffect, useState } from "react";
import type { DerivedSyncStatus } from "../hooks/useSyncStatus";
import type { ReconnectProgress } from "../store/sync";
import styles from "./OfflineBanner.module.css";

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
  const showReconnectProgress = shouldShowReconnectProgress(
    status,
    reconnectProgress,
  );
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
      className={clsx(styles.banner, visible ? styles.visible : styles.hidden)}
      data-testid="offline-banner"
      role="status"
    >
      {visible ? (
        <div className={styles.content}>
          <span data-testid="offline-banner-message">{message}</span>
          {showOffline ? (
            <button
              aria-label="Dismiss offline banner"
              className={styles.dismissButton}
              data-testid="offline-banner-dismiss"
              onClick={() => setDismissed(true)}
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
