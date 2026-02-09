import type { ChangeEvent, FormEvent } from "react";
import controls from "../styles/Controls.module.css";
import styles from "./Layout.module.css";

const TAG_SUGGESTION_LIST_ID = "workspace-tag-suggestions";

export interface AddTagDialogProps {
  documentPath: string | null;
  onCancel: () => void;
  onConfirm: () => void;
  onTagChange: (value: string) => void;
  open: boolean;
  suggestions: readonly string[];
  tagValue: string;
}

export function AddTagDialog({
  documentPath,
  onCancel,
  onConfirm,
  onTagChange,
  open,
  suggestions,
  tagValue,
}: AddTagDialogProps) {
  if (!open) {
    return null;
  }

  return (
    <div
      className={styles.deleteOverlay}
      data-motion="enter"
      data-testid="add-tag-overlay"
    >
      <div
        aria-label="Add tag dialog"
        aria-modal="true"
        className={styles.deleteDialog}
        data-motion="enter"
        data-testid="add-tag-dialog"
        role="dialog"
      >
        <h2 className={styles.deleteDialogTitle}>Add tag</h2>
        <p className={styles.deleteDialogDescription}>
          Add a tag for <strong>{documentPath}</strong>.
        </p>
        <form
          onSubmit={(event: FormEvent<HTMLFormElement>) => {
            event.preventDefault();
            onConfirm();
          }}
        >
          <div className={styles.tagField}>
            <label className={styles.tagLabel} htmlFor="add-tag-input">
              Tag
            </label>
            <input
              className={`${controls.textInput} ${styles.tagInput}`}
              data-testid="add-tag-input"
              id="add-tag-input"
              list={TAG_SUGGESTION_LIST_ID}
              onChange={(event: ChangeEvent<HTMLInputElement>) =>
                onTagChange(event.target.value)
              }
              type="text"
              value={tagValue}
            />
            <datalist id={TAG_SUGGESTION_LIST_ID}>
              {suggestions.map((suggestion) => (
                <option key={suggestion} value={suggestion} />
              ))}
            </datalist>
          </div>
          <div className={styles.deleteDialogActions}>
            <button
              className={styles.secondaryButton}
              data-testid="add-tag-cancel"
              onClick={onCancel}
              type="button"
            >
              Cancel
            </button>
            <button
              className={styles.primaryButton}
              data-testid="add-tag-confirm"
              type="submit"
            >
              Add tag
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
