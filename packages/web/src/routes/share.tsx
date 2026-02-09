import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  ApiClientError,
  redeemShareLink as redeemShareLinkApi,
} from "../lib/api-client";
import controls from "../styles/Controls.module.css";
import styles from "./share.module.css";

type RedeemStatus =
  | "idle"
  | "redeeming"
  | "password_required"
  | "error"
  | "success";

function redeemErrorMessage(error: unknown): string {
  if (error instanceof ApiClientError) {
    switch (error.code) {
      case "SHARE_LINK_DISABLED":
        return "This share link has been disabled.";
      case "SHARE_LINK_EXPIRED":
        return "This share link has expired.";
      case "SHARE_LINK_EXHAUSTED":
        return "This share link has reached its maximum uses.";
      case "SHARE_LINK_INVALID_TOKEN":
        return "Share link is invalid.";
      case "RATE_LIMITED":
        return "Too many redeem attempts. Please try again shortly.";
      default:
        return error.message || "Failed to redeem share link.";
    }
  }

  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }
  return "Failed to redeem share link.";
}

export function ShareRedeemRoute() {
  const { shareToken } = useParams<{ shareToken: string }>();
  const navigate = useNavigate();
  const [status, setStatus] = useState<RedeemStatus>("idle");
  const [passwordInput, setPasswordInput] = useState("");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [remainingUses, setRemainingUses] = useState<number | null>(null);
  const passwordInputRef = useRef<HTMLInputElement | null>(null);

  const normalizedToken = useMemo(
    () => (typeof shareToken === "string" ? shareToken.trim() : ""),
    [shareToken],
  );
  const hasValidToken = normalizedToken.length > 0;

  const redeem = useCallback(
    async (password?: string) => {
      if (!hasValidToken) {
        return;
      }

      setStatus("redeeming");
      setErrorMessage(null);

      try {
        const payload = await redeemShareLinkApi({
          token: normalizedToken,
          ...(password ? { password } : {}),
        });
        setStatus("success");
        setRemainingUses(payload.remaining_uses);
        const workspaceSegment = encodeURIComponent(payload.workspace_id);
        const targetSegment = encodeURIComponent(payload.target_id);
        const destination =
          payload.target_type === "document"
            ? `/workspace/${workspaceSegment}/document/${targetSegment}`
            : `/workspace/${workspaceSegment}`;
        navigate(destination, { replace: true });
      } catch (error) {
        if (
          error instanceof ApiClientError &&
          error.code === "SHARE_LINK_PASSWORD_REQUIRED"
        ) {
          setStatus("password_required");
          setErrorMessage("This share link requires a password.");
          return;
        }
        setStatus("error");
        setErrorMessage(redeemErrorMessage(error));
      }
    },
    [hasValidToken, navigate, normalizedToken],
  );

  useEffect(() => {
    if (!hasValidToken) {
      return;
    }
    void redeem();
  }, [hasValidToken, redeem]);

  if (!hasValidToken) {
    return (
      <main aria-label="Share link redemption" className={styles.page}>
        <h1 className={styles.title} data-testid="share-redeem-title">
          Share link redemption
        </h1>
        <p className={styles.invalid} data-testid="share-redeem-invalid">
          Share link is invalid.
        </p>
      </main>
    );
  }

  return (
    <main aria-label="Share link redemption" className={styles.page}>
      <h1 className={styles.title} data-testid="share-redeem-title">
        Share link redemption
      </h1>
      <p className={styles.metaRow} data-testid="share-redeem-token">
        Token: {normalizedToken}
      </p>

      {status === "redeeming" ? (
        <p className={styles.metaRow} data-testid="share-redeem-loading">
          Redeeming share link…
        </p>
      ) : null}

      {status === "password_required" ? (
        <>
          <label className={styles.metaRow} htmlFor="share-redeem-password">
            Password
          </label>
          <input
            className={styles.passwordInput}
            data-testid="share-redeem-password"
            id="share-redeem-password"
            onChange={(event) => setPasswordInput(event.target.value)}
            ref={passwordInputRef}
            type="password"
            value={passwordInput}
          />
          <button
            className={`${controls.buttonBase} ${controls.buttonPrimary}`}
            data-testid="share-redeem-submit"
            onClick={() => {
              const nextPassword =
                passwordInputRef.current?.value.trim() ?? passwordInput.trim();
              void redeem(nextPassword.length > 0 ? nextPassword : undefined);
            }}
            type="button"
          >
            Redeem link
          </button>
        </>
      ) : null}

      {status === "error" && errorMessage ? (
        <p className={styles.invalid} data-testid="share-redeem-error">
          {errorMessage}
        </p>
      ) : null}

      {status === "success" ? (
        <p className={styles.success} data-testid="share-redeem-success">
          Share link redeemed. Redirecting…
        </p>
      ) : null}

      {status === "success" && remainingUses !== null ? (
        <p className={styles.metaRow} data-testid="share-redeem-remaining-uses">
          Remaining uses: {remainingUses}
        </p>
      ) : null}

      {status === "password_required" && errorMessage ? (
        <p
          className={styles.unavailable}
          data-testid="share-redeem-password-required"
        >
          {errorMessage}
        </p>
      ) : null}

      {status === "idle" ? (
        <button
          className={`${controls.buttonBase} ${controls.buttonPrimary}`}
          data-testid="share-redeem-submit"
          onClick={() => {
            void redeem();
          }}
          type="button"
        >
          Redeem link
        </button>
      ) : null}
    </main>
  );
}
