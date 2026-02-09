import type { Document } from "@scriptum/shared";
import type { NavigateFunction } from "react-router-dom";
import type { ContextMenuAction } from "../components/sidebar/DocumentTree";
import { buildUntitledPath, titleFromPath } from "../lib/document-utils";
import { rewriteWikiReferencesForRename } from "../lib/wiki-links";

interface ToastApi {
  error(message: string): void;
  success(message: string): void;
}

export interface UseDocumentCrudOptions {
  activeDocumentId: string | null;
  activeWorkspaceId: string | null;
  documents: readonly Document[];
  formatRenameBacklinkToast: (
    updatedLinks: number,
    updatedDocuments: number,
  ) => string;
  navigate: NavigateFunction;
  openDocument: (documentId: string) => void;
  pendingDeleteDocument: Document | null;
  removeDocument: (documentId: string) => void;
  setActiveDocumentForWorkspace: (
    workspaceId: string,
    documentId: string | null,
  ) => void;
  setPendingDeleteDocument: (document: Document | null) => void;
  setPendingRenameDocumentId: (documentId: string | null) => void;
  toast: ToastApi;
  upsertDocument: (document: Document) => void;
  workspaceDocuments: readonly Document[];
}

export interface UseDocumentCrudResult {
  createDocumentInActiveWorkspace: (
    path: string,
    options?: { inlineRename?: boolean },
  ) => Document | null;
  createUntitledDocument: () => void;
  handleCancelDeleteDocument: () => void;
  handleConfirmDeleteDocument: () => void;
  handleDocumentContextAction: (
    action: ContextMenuAction,
    document: Document,
  ) => void;
  handleRenameDocument: (documentId: string, nextPath: string) => void;
}

export function useDocumentCrud(
  options: UseDocumentCrudOptions,
): UseDocumentCrudResult {
  const {
    activeDocumentId,
    activeWorkspaceId,
    documents,
    formatRenameBacklinkToast,
    navigate,
    openDocument,
    pendingDeleteDocument,
    removeDocument,
    setActiveDocumentForWorkspace,
    setPendingDeleteDocument,
    setPendingRenameDocumentId,
    toast,
    upsertDocument,
    workspaceDocuments,
  } = options;

  const createDocumentInActiveWorkspace = (
    path: string,
    createOptions: { inlineRename?: boolean } = {},
  ): Document | null => {
    if (!activeWorkspaceId) {
      return null;
    }

    const token = `${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6)
      .toString(36)
      .padStart(4, "0")}`;
    const now = new Date().toISOString();
    const document: Document = {
      archivedAt: null,
      bodyMd: "",
      createdAt: now,
      deletedAt: null,
      etag: `document-${token}`,
      headSeq: 0,
      id: `doc-${token}`,
      path,
      tags: [],
      title: titleFromPath(path),
      updatedAt: now,
      workspaceId: activeWorkspaceId,
    };

    upsertDocument(document);
    openDocument(document.id);
    setActiveDocumentForWorkspace(activeWorkspaceId, document.id);
    navigate(
      `/workspace/${encodeURIComponent(activeWorkspaceId)}/document/${encodeURIComponent(document.id)}`,
    );

    if (createOptions.inlineRename) {
      setPendingRenameDocumentId(document.id);
    }

    return document;
  };

  const createUntitledDocument = () => {
    const existingPaths = new Set(
      workspaceDocuments.map((document) => document.path),
    );
    createDocumentInActiveWorkspace(buildUntitledPath(existingPaths), {
      inlineRename: true,
    });
  };

  const handleRenameDocument = (documentId: string, nextPath: string) => {
    const normalizedPath = nextPath.trim();
    if (!normalizedPath) {
      return;
    }

    const currentDocument = documents.find(
      (document) => document.id === documentId,
    );
    if (!currentDocument) {
      return;
    }

    const { rewrittenDocuments, updatedDocuments, updatedLinks } =
      rewriteWikiReferencesForRename(
        workspaceDocuments,
        currentDocument,
        normalizedPath,
      );

    const now = new Date().toISOString();
    const mutationToken = Date.now().toString(36);

    upsertDocument({
      ...currentDocument,
      etag: `${currentDocument.etag}:rename:${mutationToken}`,
      path: normalizedPath,
      title: titleFromPath(normalizedPath),
      updatedAt: now,
    });

    for (const rewrittenDocument of rewrittenDocuments) {
      upsertDocument({
        ...rewrittenDocument,
        etag: `${rewrittenDocument.etag}:backlink-rename:${mutationToken}`,
        updatedAt: now,
      });
    }

    toast.success(formatRenameBacklinkToast(updatedLinks, updatedDocuments));
    setPendingRenameDocumentId(null);
  };

  const updateExistingDocument = (
    documentId: string,
    updater: (document: Document) => Document,
  ) => {
    const currentDocument = documents.find(
      (document) => document.id === documentId,
    );
    if (!currentDocument) {
      return;
    }
    upsertDocument(updater(currentDocument));
  };

  const createDocumentInNewFolder = (sourceDocument: Document) => {
    const segments = sourceDocument.path.split("/").filter(Boolean);
    const parentPath = segments.slice(0, -1).join("/");
    const existingPaths = new Set(
      workspaceDocuments.map((document) => document.path),
    );
    let suffix = 1;
    let folderName = "new-folder";
    let candidatePath = `${folderName}/untitled.md`;

    while (
      existingPaths.has(
        parentPath.length > 0
          ? `${parentPath}/${candidatePath}`
          : candidatePath,
      )
    ) {
      suffix += 1;
      folderName = `new-folder-${suffix}`;
      candidatePath = `${folderName}/untitled.md`;
    }

    createDocumentInActiveWorkspace(
      parentPath.length > 0 ? `${parentPath}/${candidatePath}` : candidatePath,
      { inlineRename: true },
    );
  };

  const handleDocumentContextAction = (
    action: ContextMenuAction,
    document: Document,
  ) => {
    if (action === "rename") {
      return;
    }

    if (action === "delete") {
      setPendingDeleteDocument(document);
      return;
    }

    if (action === "move") {
      const fileName = titleFromPath(document.path);
      handleRenameDocument(document.id, `moved/${fileName}`);
      return;
    }

    if (action === "copy-link") {
      const link = `/workspace/${encodeURIComponent(document.workspaceId)}/document/${encodeURIComponent(document.id)}`;
      if (typeof navigator !== "undefined" && navigator.clipboard) {
        void navigator.clipboard
          .writeText(link)
          .then(() => {
            toast.success("Copied document link.");
          })
          .catch(() => {
            toast.error("Failed to copy document link.");
          });
      } else {
        toast.error("Clipboard is unavailable in this environment.");
      }
      return;
    }

    if (action === "add-tag") {
      const now = new Date().toISOString();
      updateExistingDocument(document.id, (currentDocument) => ({
        ...currentDocument,
        etag: `${currentDocument.etag}:tag:${Date.now().toString(36)}`,
        tags: Array.from(new Set([...currentDocument.tags, "tagged"])),
        updatedAt: now,
      }));
      return;
    }

    if (action === "archive") {
      const now = new Date().toISOString();
      updateExistingDocument(document.id, (currentDocument) => ({
        ...currentDocument,
        archivedAt: now,
        etag: `${currentDocument.etag}:archive:${Date.now().toString(36)}`,
        updatedAt: now,
      }));
      return;
    }

    if (action === "unarchive") {
      const now = new Date().toISOString();
      updateExistingDocument(document.id, (currentDocument) => ({
        ...currentDocument,
        archivedAt: null,
        etag: `${currentDocument.etag}:unarchive:${Date.now().toString(36)}`,
        updatedAt: now,
      }));
      return;
    }

    if (action === "new-folder") {
      createDocumentInNewFolder(document);
      return;
    }
  };

  const handleCancelDeleteDocument = () => {
    setPendingDeleteDocument(null);
  };

  const handleConfirmDeleteDocument = () => {
    const documentToDelete = pendingDeleteDocument;
    if (!documentToDelete) {
      return;
    }

    const deletingActiveDocument = activeDocumentId === documentToDelete.id;
    removeDocument(documentToDelete.id);
    setPendingDeleteDocument(null);
    toast.success(`Deleted "${documentToDelete.path}".`);

    if (
      !deletingActiveDocument ||
      !activeWorkspaceId ||
      documentToDelete.workspaceId !== activeWorkspaceId
    ) {
      return;
    }

    const nextDocumentId =
      workspaceDocuments.find((document) => document.id !== documentToDelete.id)
        ?.id ?? null;

    setActiveDocumentForWorkspace(activeWorkspaceId, nextDocumentId);
    if (nextDocumentId) {
      navigate(
        `/workspace/${encodeURIComponent(activeWorkspaceId)}/document/${encodeURIComponent(nextDocumentId)}`,
      );
      return;
    }

    navigate(`/workspace/${encodeURIComponent(activeWorkspaceId)}`);
  };

  return {
    createDocumentInActiveWorkspace,
    createUntitledDocument,
    handleCancelDeleteDocument,
    handleConfirmDeleteDocument,
    handleDocumentContextAction,
    handleRenameDocument,
  };
}
