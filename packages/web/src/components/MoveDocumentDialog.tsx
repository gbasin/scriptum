import clsx from "clsx";
import type { Document } from "@scriptum/shared";
import { DocumentTree } from "./sidebar/DocumentTree";
import styles from "./Layout.module.css";

export interface MoveDocumentDialogProps {
  destinationFolderPath: string | null;
  documentPath: string | null;
  onCancel: () => void;
  onConfirm: () => void;
  onDestinationFolderChange: (path: string) => void;
  open: boolean;
  workspaceDocuments: readonly Document[];
}

export function MoveDocumentDialog({
  destinationFolderPath,
  documentPath,
  onCancel,
  onConfirm,
  onDestinationFolderChange,
  open,
  workspaceDocuments,
}: MoveDocumentDialogProps) {
  if (!open) {
    return null;
  }

  return (
    <div className={styles.deleteOverlay} data-testid="move-document-overlay">
      <div
        aria-label="Move document dialog"
        aria-modal="true"
        className={styles.deleteDialog}
        data-testid="move-document-dialog"
        role="dialog"
      >
        <h2 className={styles.deleteDialogTitle}>Move document</h2>
        <p className={styles.deleteDialogDescription}>
          Select a destination folder for <strong>{documentPath}</strong>.
        </p>
        <div className={styles.movePicker}>
          <button
            className={clsx(
              styles.secondaryButton,
              destinationFolderPath === "" && styles.secondaryButtonActive,
            )}
            data-testid="move-destination-root"
            onClick={() => onDestinationFolderChange("")}
            type="button"
          >
            Workspace root
          </button>
          <DocumentTree
            activeDocumentId={null}
            documents={workspaceDocuments.slice()}
            onFolderSelect={onDestinationFolderChange}
            selectedFolderPath={destinationFolderPath}
          />
        </div>
        <div className={styles.deleteDialogActions}>
          <button
            className={styles.secondaryButton}
            data-testid="move-document-cancel"
            onClick={onCancel}
            type="button"
          >
            Cancel
          </button>
          <button
            className={styles.primaryButton}
            data-testid="move-document-confirm"
            disabled={destinationFolderPath === null}
            onClick={onConfirm}
            type="button"
          >
            Move
          </button>
        </div>
      </div>
    </div>
  );
}
