// OAuth callback handler — exchanges authorization code for tokens.

import { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { AuthClient } from "../auth/client";
import { useAuthStore } from "../store/auth";
import { isFixtureModeEnabled } from "../test/setup";
import styles from "./auth-callback.module.css";

const RELAY_URL =
  import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";
export const AUTH_CALLBACK_TIMEOUT_MS = 10_000;

export function AuthCallbackRoute() {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const handleCallback = useAuthStore((s) => s.handleCallback);
  const status = useAuthStore((s) => s.status);
  const error = useAuthStore((s) => s.error);
  const fixtureModeEnabled = isFixtureModeEnabled();
  const startedAttempt = useRef<number | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [timedOut, setTimedOut] = useState(false);

  useEffect(() => {
    if (fixtureModeEnabled) {
      return;
    }
    if (startedAttempt.current === attempt) {
      return;
    }
    startedAttempt.current = attempt;

    const code = searchParams.get("code");
    const state = searchParams.get("state");

    if (!code || !state) {
      navigate("/", { replace: true });
      return;
    }

    setTimedOut(false);
    const client = new AuthClient({ baseUrl: RELAY_URL });
    const timeoutHandle = window.setTimeout(() => {
      setTimedOut(true);
    }, AUTH_CALLBACK_TIMEOUT_MS);

    void handleCallback(client, code, state).finally(() => {
      window.clearTimeout(timeoutHandle);
    });

    return () => {
      window.clearTimeout(timeoutHandle);
    };
  }, [attempt, fixtureModeEnabled, handleCallback, navigate, searchParams]);

  useEffect(() => {
    if (fixtureModeEnabled) {
      return;
    }
    if (status === "authenticated") {
      navigate("/", { replace: true });
    }
  }, [fixtureModeEnabled, status, navigate]);

  if (error) {
    return (
      <section className={styles.page} data-testid="auth-callback-error">
        <h1 className={styles.title}>Auth Callback</h1>
        <p className={styles.error} role="alert">
          {error}
        </p>
        <a className={styles.link} href="/">
          Return home
        </a>
      </section>
    );
  }

  if (timedOut) {
    return (
      <section className={styles.page} data-testid="auth-callback-timeout">
        <h1 className={styles.title}>Auth Callback</h1>
        <p className={styles.error} role="alert">
          Sign-in is taking longer than expected.
        </p>
        <div className={styles.actions}>
          <button
            className={styles.retryButton}
            data-testid="auth-callback-retry"
            onClick={() => setAttempt((value) => value + 1)}
            type="button"
          >
            Try again
          </button>
          <a className={styles.link} href="/">
            Return home
          </a>
        </div>
      </section>
    );
  }

  return (
    <section className={styles.page}>
      <h1 className={styles.title}>Auth Callback</h1>
      <p className={styles.pending}>Completing sign-in...</p>
    </section>
  );
}
