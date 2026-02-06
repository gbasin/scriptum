// OAuth callback handler — exchanges authorization code for tokens.

import { useEffect, useRef } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { AuthClient } from "../auth/client";
import { useAuthStore } from "../store/auth";

const RELAY_URL = import.meta.env.VITE_SCRIPTUM_RELAY_URL ?? "http://localhost:8080";

export function AuthCallbackRoute() {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const handleCallback = useAuthStore((s) => s.handleCallback);
  const status = useAuthStore((s) => s.status);
  const error = useAuthStore((s) => s.error);
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;

    const code = searchParams.get("code");
    const state = searchParams.get("state");

    if (!code || !state) {
      navigate("/", { replace: true });
      return;
    }

    const client = new AuthClient({ baseUrl: RELAY_URL });
    void handleCallback(client, code, state);
  }, [searchParams, handleCallback, navigate]);

  useEffect(() => {
    if (status === "authenticated") {
      navigate("/", { replace: true });
    }
  }, [status, navigate]);

  if (error) {
    return (
      <section data-testid="auth-callback-error">
        <h1>Auth Callback</h1>
        <p role="alert">{error}</p>
        <a href="/">Return home</a>
      </section>
    );
  }

  return (
    <section>
      <h1>Auth Callback</h1>
      <p>Completing sign-in...</p>
    </section>
  );
}
