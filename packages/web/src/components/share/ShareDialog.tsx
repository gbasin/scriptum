import type {
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
  onExpirationOptionChange: (option: ShareLinkExpirationOption) => void;
  onGenerate: () => void | Promise<void>;
  onMaxUsesInputChange: (value: string) => void;
  onPermissionChange: (permission: ShareLinkPermission) => void;
  onTargetTypeChange: (targetType: ShareLinkTargetType) => void;
  shareExpirationOption: ShareLinkExpirationOption;
  shareMaxUsesInput: string;
  sharePermission: ShareLinkPermission;
  shareTargetType: ShareLinkTargetType;
  summaryPermissionLabel: string;
}

export function ShareDialog({
  documentId,
  generationError = null,
  generatedShareUrl,
  onClose,
  onExpirationOptionChange,
  onGenerate,
  onMaxUsesInputChange,
  onPermissionChange,
  onTargetTypeChange,
  shareExpirationOption,
  shareMaxUsesInput,
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
          <input
            data-testid="share-link-url"
            readOnly
            value={generatedShareUrl}
          />
        </div>
      ) : null}
    </section>
  );
}
