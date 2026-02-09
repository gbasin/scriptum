import { AlertDialog } from "@base-ui-components/react/alert-dialog";
import styles from "./Layout.module.css";

export interface DeleteDocumentDialogProps {
  documentPath: string | null;
  onCancel: () => void;
  onConfirm: () => void;
  open: boolean;
}

export function DeleteDocumentDialog({
  documentPath,
  onCancel,
  onConfirm,
  open,
}: DeleteDocumentDialogProps) {
  return (
    <AlertDialog.Root
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          onCancel();
        }
      }}
      open={open}
    >
      <AlertDialog.Portal>
        <AlertDialog.Backdrop
          className={styles.deleteOverlay}
          data-motion={open ? "enter" : "exit"}
          data-testid="delete-document-overlay"
        />
        <AlertDialog.Popup
          aria-label="Delete document confirmation"
          className={styles.deleteDialog}
          data-motion={open ? "enter" : "exit"}
          data-testid="delete-document-dialog"
        >
          <AlertDialog.Title className={styles.deleteDialogTitle}>
            Delete document?
          </AlertDialog.Title>
          <AlertDialog.Description className={styles.deleteDialogDescription}>
            Permanently delete <strong>{documentPath}</strong>? This cannot be
            undone.
          </AlertDialog.Description>
          <div className={styles.deleteDialogActions}>
            <AlertDialog.Close
              className={styles.secondaryButton}
              data-testid="delete-document-cancel"
            >
              Cancel
            </AlertDialog.Close>
            <button
              className={styles.dangerButton}
              data-testid="delete-document-confirm"
              onClick={onConfirm}
              type="button"
            >
              Delete
            </button>
          </div>
        </AlertDialog.Popup>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
