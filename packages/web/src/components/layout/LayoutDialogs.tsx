import type { Document } from "@scriptum/shared";
import { AddTagDialog } from "../AddTagDialog";
import { DeleteDocumentDialog } from "../DeleteDocumentDialog";
import { MoveDocumentDialog } from "../MoveDocumentDialog";

export interface LayoutDialogsProps {
  pendingDeleteDocument: Document | null;
  pendingMoveDestination: string | null;
  pendingMoveDocument: Document | null;
  pendingTagDocument: Document | null;
  pendingTagValue: string;
  workspaceDocuments: readonly Document[];
  workspaceTags: readonly string[];
  onCancelAddTag: () => void;
  onCancelDeleteDocument: () => void;
  onCancelMoveDocument: () => void;
  onConfirmAddTag: () => void;
  onConfirmDeleteDocument: () => void;
  onConfirmMoveDocument: () => void;
  onMoveDestinationChange: (path: string) => void;
  onTagChange: (value: string) => void;
}

export function LayoutDialogs({
  pendingDeleteDocument,
  pendingMoveDestination,
  pendingMoveDocument,
  pendingTagDocument,
  pendingTagValue,
  workspaceDocuments,
  workspaceTags,
  onCancelAddTag,
  onCancelDeleteDocument,
  onCancelMoveDocument,
  onConfirmAddTag,
  onConfirmDeleteDocument,
  onConfirmMoveDocument,
  onMoveDestinationChange,
  onTagChange,
}: LayoutDialogsProps) {
  return (
    <>
      <DeleteDocumentDialog
        documentPath={pendingDeleteDocument?.path ?? null}
        onCancel={onCancelDeleteDocument}
        onConfirm={onConfirmDeleteDocument}
        open={Boolean(pendingDeleteDocument)}
      />
      <MoveDocumentDialog
        destinationFolderPath={pendingMoveDestination}
        documentPath={pendingMoveDocument?.path ?? null}
        onCancel={onCancelMoveDocument}
        onConfirm={onConfirmMoveDocument}
        onDestinationFolderChange={onMoveDestinationChange}
        open={Boolean(pendingMoveDocument)}
        workspaceDocuments={workspaceDocuments}
      />
      <AddTagDialog
        documentPath={pendingTagDocument?.path ?? null}
        onCancel={onCancelAddTag}
        onConfirm={onConfirmAddTag}
        onTagChange={onTagChange}
        open={Boolean(pendingTagDocument)}
        suggestions={workspaceTags}
        tagValue={pendingTagValue}
      />
    </>
  );
}
