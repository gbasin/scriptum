import clsx from "clsx";
import { useEffect, useRef } from "react";
import { useToastStore } from "../store/toast";
import styles from "./ToastViewport.module.css";

export function ToastViewport() {
  const toasts = useToastStore((state) => state.toasts);
  const dismissToast = useToastStore((state) => state.dismissToast);
  const timeoutByToastIdRef = useRef<Map<string, number>>(new Map());

  useEffect(() => {
    for (const toast of toasts) {
      if (timeoutByToastIdRef.current.has(toast.id)) {
        continue;
      }

      const timeoutId = window.setTimeout(() => {
        dismissToast(toast.id);
        timeoutByToastIdRef.current.delete(toast.id);
      }, toast.durationMs);
      timeoutByToastIdRef.current.set(toast.id, timeoutId);
    }

    const activeToastIds = new Set(toasts.map((toast) => toast.id));
    for (const [toastId, timeoutId] of timeoutByToastIdRef.current) {
      if (activeToastIds.has(toastId)) {
        continue;
      }
      window.clearTimeout(timeoutId);
      timeoutByToastIdRef.current.delete(toastId);
    }

    return () => {
      for (const timeoutId of timeoutByToastIdRef.current.values()) {
        window.clearTimeout(timeoutId);
      }
      timeoutByToastIdRef.current.clear();
    };
  }, [dismissToast, toasts]);

  if (toasts.length === 0) {
    return null;
  }

  return (
    <section
      aria-label="Notifications"
      className={styles.viewport}
      data-testid="toast-viewport"
    >
      {toasts.map((toast) => (
        <article
          className={styles.toast}
          data-motion="enter"
          data-testid="toast-item"
          key={toast.id}
          role={toast.variant === "error" ? "alert" : "status"}
        >
          <span
            aria-hidden="true"
            className={clsx(
              styles.marker,
              toast.variant === "success" && styles.markerSuccess,
              toast.variant === "error" && styles.markerError,
              toast.variant === "info" && styles.markerInfo,
            )}
          />
          <p className={styles.message}>{toast.message}</p>
          <button
            aria-label="Dismiss notification"
            className={styles.dismissButton}
            data-testid={`toast-dismiss-${toast.id}`}
            onClick={() => dismissToast(toast.id)}
            type="button"
          >
            x
          </button>
        </article>
      ))}
    </section>
  );
}
