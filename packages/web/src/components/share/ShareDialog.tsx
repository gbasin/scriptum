import type {
  RelayShareLinkRecord,
  ShareLinkExpirationOption,
  ShareLinkPermission,
  ShareLinkTargetType,
} from "../../routes/share-links";
import styles from "./ShareDialog.module.css";

interface ShareDialogProps {
  documentId: string | undefined;
  generationError?: string | null;
  generatedShareUrl: string;
  onClose: () => void;
  onCopyGeneratedUrl?: () => void | Promise<void>;
  onExpirationOptionChange: (option: ShareLinkExpirationOption) => void;
  onGenerate: () => void | Promise<void>;
  onMaxUsesInputChange: (value: string) => void;
  onPasswordInputChange: (value: string) => void;
  onPermissionChange: (permission: ShareLinkPermission) => void;
  onRevokeShareLink?: (shareLinkId: string) => void | Promise<void>;
  onTargetTypeChange: (targetType: ShareLinkTargetType) => void;
  existingShareLinks?: RelayShareLinkRecord[];
  existingShareLinksError?: string | null;
  isLoadingExistingShareLinks?: boolean;
  isRevokingShareLinkId?: string | null;
  shareExpirationOption: ShareLinkExpirationOption;
  shareMaxUsesInput: string;
  sharePasswordInput: string;
  sharePermission: ShareLinkPermission;
  shareTargetType: ShareLinkTargetType;
  summaryPermissionLabel: string;
}

function formatExpiration(expiresAt: string | null): string {
  if (!expiresAt) {
    return "Never";
  }
  const parsed = new Date(expiresAt);
  if (Number.isNaN(parsed.valueOf())) {
    return "Invalid";
  }
  return parsed.toISOString();
}

export function ShareDialog({
  documentId,
  existingShareLinks = [],
  existingShareLinksError = null,
  generationError = null,
  generatedShareUrl,
  isLoadingExistingShareLinks = false,
  isRevokingShareLinkId = null,
  onClose,
  onCopyGeneratedUrl,
  onExpirationOptionChange,
  onGenerate,
  onMaxUsesInputChange,
  onPasswordInputChange,
  onPermissionChange,
  onRevokeShareLink,
  onTargetTypeChange,
  shareExpirationOption,
  shareMaxUsesInput,
  sharePasswordInput,
  sharePermission,
  shareTargetType,
  summaryPermissionLabel,
}: ShareDialogProps) {
  return (
    <section
      aria-label="Share link dialog"
      className={styles.dialog}
      data-testid="share-link-dialog"
    >
      <label htmlFor="share-link-target">Target</label>
      <select
        data-testid="share-link-target"
        id="share-link-target"
        onChange={(event) =>
          onTargetTypeChange(
            event.target.value === "workspace" ? "workspace" : "document",
          )
        }
        value={shareTargetType}
      >
        <option value="workspace">Workspace</option>
        <option disabled={!documentId} value="document">
          Document
        </option>
      </select>

      <label htmlFor="share-link-permission">Permission</label>
      <select
        data-testid="share-link-permission"
        id="share-link-permission"
        onChange={(event) =>
          onPermissionChange(event.target.value === "edit" ? "edit" : "view")
        }
        value={sharePermission}
      >
        <option value="view">Viewer</option>
        <option value="edit">Editor</option>
      </select>

      <label htmlFor="share-link-expiration">Expiration</label>
      <select
        data-testid="share-link-expiration"
        id="share-link-expiration"
        onChange={(event) =>
          onExpirationOptionChange(
            event.target.value === "24h"
              ? "24h"
              : event.target.value === "7d"
                ? "7d"
                : "none",
          )
        }
        value={shareExpirationOption}
      >
        <option value="none">Never</option>
        <option value="24h">24 hours</option>
        <option value="7d">7 days</option>
      </select>

      <label htmlFor="share-link-max-uses">Max uses</label>
      <input
        data-testid="share-link-max-uses"
        id="share-link-max-uses"
        min={1}
        onChange={(event) => onMaxUsesInputChange(event.target.value)}
        type="number"
        value={shareMaxUsesInput}
      />

      <label htmlFor="share-link-password">Password (optional)</label>
      <input
        data-testid="share-link-password"
        id="share-link-password"
        onChange={(event) => onPasswordInputChange(event.target.value)}
        placeholder="Require password to redeem"
        type="password"
        value={sharePasswordInput}
      />

      <div className={styles.actions}>
        <button data-testid="share-link-close" onClick={onClose} type="button">
          Close
        </button>
        <button
          data-testid="share-link-generate"
          onClick={onGenerate}
          type="button"
        >
          Generate link
        </button>
      </div>

      {generationError ? (
        <p className={styles.errorMessage} data-testid="share-link-error">
          {generationError}
        </p>
      ) : null}

      {generatedShareUrl ? (
        <div className={styles.generatedPanel}>
          <p data-testid="share-link-summary">
            Link grants {summaryPermissionLabel} access to{" "}
            {shareTargetType === "workspace" ? "workspace" : "document"}.
          </p>
          <div className={styles.generatedRow}>
            <input
              data-testid="share-link-url"
              readOnly
              value={generatedShareUrl}
            />
            <button
              data-testid="share-link-copy"
              onClick={onCopyGeneratedUrl}
              type="button"
            >
              Copy
            </button>
          </div>
        </div>
      ) : null}

      <section className={styles.existingSection}>
        <h3>Existing links</h3>
        {isLoadingExistingShareLinks ? (
          <p data-testid="share-link-existing-loading">Loading share links…</p>
        ) : null}
        {existingShareLinksError ? (
          <p
            className={styles.errorMessage}
            data-testid="share-link-existing-error"
          >
            {existingShareLinksError}
          </p>
        ) : null}
        {!isLoadingExistingShareLinks &&
        !existingShareLinksError &&
        existingShareLinks.length === 0 ? (
          <p data-testid="share-link-existing-empty">No active share links.</p>
        ) : null}
        {existingShareLinks.length > 0 ? (
          <ul
            className={styles.existingList}
            data-testid="share-link-existing-list"
          >
            {existingShareLinks.map((shareLink) => (
              <li key={shareLink.id}>
                <p>
                  {shareLink.targetType} / {shareLink.targetId}
                </p>
                <p>
                  {shareLink.permission === "edit" ? "Editor" : "Viewer"} •
                  Expires {formatExpiration(shareLink.expiresAt)} • Uses{" "}
                  {shareLink.useCount}
                  {shareLink.maxUses === null ? "" : `/${shareLink.maxUses}`}
                </p>
                <p>{shareLink.disabled ? "Disabled" : "Active"}</p>
                <button
                  data-testid={`share-link-revoke-${shareLink.id}`}
                  disabled={
                    shareLink.disabled || isRevokingShareLinkId === shareLink.id
                  }
                  onClick={() => onRevokeShareLink?.(shareLink.id)}
                  type="button"
                >
                  {isRevokingShareLinkId === shareLink.id
                    ? "Revoking…"
                    : "Revoke"}
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </section>
    </section>
  );
}
