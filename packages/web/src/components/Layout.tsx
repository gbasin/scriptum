import { AlertDialog } from "@base-ui-components/react/alert-dialog";
import type { Document, Workspace } from "@scriptum/shared";
import { useEffect, useMemo, useRef, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import {
  buildIncomingBacklinks,
  type IncomingBacklink,
  rewriteWikiReferencesForRename,
  type RenameBacklinkRewriteResult,
} from "../lib/wiki-links";
import { useToast } from "../hooks/useToast";
import { useDocumentsStore } from "../store/documents";
import { usePresenceStore } from "../store/presence";
import { useUiStore } from "../store/ui";
import { useWorkspaceStore } from "../store/workspace";
import { CommandPalette } from "./CommandPalette";
import styles from "./Layout.module.css";
import { Backlinks } from "./right-panel/Backlinks";
import { Outline } from "./right-panel/Outline";
import { ToastViewport } from "./ToastViewport";
import { AgentsSection } from "./sidebar/AgentsSection";
import { type ContextMenuAction, DocumentTree } from "./sidebar/DocumentTree";
import {
  buildSearchPanelResults,
  isSearchPanelShortcut,
  SearchPanel,
} from "./sidebar/SearchPanel";
import {
  collectWorkspaceTags,
  filterDocumentsByTag,
  TagsList,
} from "./sidebar/TagsList";
import { WorkspaceDropdown } from "./sidebar/WorkspaceDropdown";
export { buildIncomingBacklinks, rewriteWikiReferencesForRename };
export type { IncomingBacklink, RenameBacklinkRewriteResult };

export function isNewDocumentShortcut(event: KeyboardEvent): boolean {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "n"
  );
}

function titleFromPath(path: string): string {
  const segments = path.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

export function formatRenameBacklinkToast(
  updatedLinks: number,
  updatedDocuments: number,
): string {
  return `Updated ${updatedLinks} links across ${updatedDocuments} documents.`;
}

const COMPACT_LAYOUT_BREAKPOINT_PX = 1024;

export function Layout() {
  const navigate = useNavigate();
  const activeWorkspaceId = useWorkspaceStore(
    (state) => state.activeWorkspaceId,
  );
  const setActiveWorkspaceId = useWorkspaceStore(
    (state) => state.setActiveWorkspaceId,
  );
  const upsertWorkspace = useWorkspaceStore((state) => state.upsertWorkspace);
  const workspaces = useWorkspaceStore((state) => state.workspaces);
  const documents = useDocumentsStore((state) => state.documents);
  const activeDocumentIdByWorkspace = useDocumentsStore(
    (state) => state.activeDocumentIdByWorkspace,
  );
  const setActiveDocumentForWorkspace = useDocumentsStore(
    (state) => state.setActiveDocumentForWorkspace,
  );
  const openDocument = useDocumentsStore((state) => state.openDocument);
  const removeDocument = useDocumentsStore((state) => state.removeDocument);
  const upsertDocument = useDocumentsStore((state) => state.upsertDocument);
  const openDocumentIds = useDocumentsStore((state) => state.openDocumentIds);
  const remotePeers = usePresenceStore((state) => state.remotePeers);
  const sidebarPanel = useUiStore((state) => state.sidebarPanel);
  const sidebarOpen = useUiStore((state) => state.sidebarOpen);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const setSidebarPanel = useUiStore((state) => state.setSidebarPanel);
  const rightPanelOpen = useUiStore((state) => state.rightPanelOpen);
  const rightPanelTab = useUiStore((state) => state.rightPanelTab);
  const toggleRightPanel = useUiStore((state) => state.toggleRightPanel);
  const setRightPanelTab = useUiStore((state) => state.setRightPanelTab);
  const commandPaletteOpen = useUiStore((state) => state.commandPaletteOpen);
  const openCommandPalette = useUiStore((state) => state.openCommandPalette);
  const closeCommandPalette = useUiStore((state) => state.closeCommandPalette);
  const toast = useToast();
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [pendingRenameDocumentId, setPendingRenameDocumentId] = useState<
    string | null
  >(null);
  const [pendingDeleteDocument, setPendingDeleteDocument] =
    useState<Document | null>(null);
  const [outlineContainer, setOutlineContainer] = useState<HTMLElement | null>(
    null,
  );
  const editorAreaRef = useRef<HTMLElement | null>(null);
  const [isCompactLayout, setIsCompactLayout] = useState(false);
  const wasCompactLayoutRef = useRef<boolean | null>(null);

  const workspaceDocuments = useMemo(
    () =>
      activeWorkspaceId
        ? documents.filter(
            (document) => document.workspaceId === activeWorkspaceId,
          )
        : [],
    [activeWorkspaceId, documents],
  );
  const workspaceTags = useMemo(
    () => collectWorkspaceTags(workspaceDocuments),
    [workspaceDocuments],
  );
  const filteredDocuments = useMemo(
    () => filterDocumentsByTag(workspaceDocuments, activeTag),
    [workspaceDocuments, activeTag],
  );
  const searchPanelResults = useMemo(
    () => buildSearchPanelResults(workspaceDocuments),
    [workspaceDocuments],
  );
  const activeDocumentId = activeWorkspaceId
    ? (activeDocumentIdByWorkspace[activeWorkspaceId] ?? null)
    : null;
  const incomingBacklinks = useMemo(
    () => buildIncomingBacklinks(workspaceDocuments, activeDocumentId),
    [workspaceDocuments, activeDocumentId],
  );
  const incomingBacklinkEntries = useMemo(
    () =>
      incomingBacklinks.map((backlink) => ({
        docId: backlink.sourceDocumentId,
        path: backlink.sourcePath,
        title: backlink.sourceTitle,
        linkText: backlink.snippet,
        snippet: backlink.snippet,
      })),
    [incomingBacklinks],
  );
  const showPanelSkeletons =
    activeWorkspaceId !== null && workspaceDocuments.length === 0;
  const searchPanelOpen = sidebarPanel === "search";
  const showOutlineSkeleton = showPanelSkeletons || outlineContainer === null;

  const createDocumentInActiveWorkspace = (
    path: string,
    options: { inlineRename?: boolean } = {},
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
      createdAt: now,
      deletedAt: null,
      etag: `document-${token}`,
      headSeq: 0,
      id: `doc-${token}`,
      bodyMd: "",
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

    if (options.inlineRename) {
      setPendingRenameDocumentId(document.id);
    }

    return document;
  };

  const createUntitledDocument = () => {
    const existingPaths = new Set(
      workspaceDocuments.map((document) => document.path),
    );
    let suffix = 1;
    let candidatePath = "untitled-1.md";

    while (existingPaths.has(candidatePath)) {
      suffix += 1;
      candidatePath = `untitled-${suffix}.md`;
    }

    createDocumentInActiveWorkspace(candidatePath, { inlineRename: true });
  };

  useEffect(() => {
    setActiveTag(null);
  }, [activeWorkspaceId]);

  useEffect(() => {
    if (activeTag && !workspaceTags.includes(activeTag)) {
      setActiveTag(null);
    }
  }, [activeTag, workspaceTags]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isNewDocumentShortcut(event)) {
        event.preventDefault();
        createUntitledDocument();
        return;
      }
      if (!isSearchPanelShortcut(event)) {
        return;
      }
      event.preventDefault();
      setSidebarPanel("search");
    };

    if (typeof window === "undefined") {
      return undefined;
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [createUntitledDocument, setSidebarPanel]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return undefined;
    }

    const syncViewportMode = () => {
      setIsCompactLayout(window.innerWidth < COMPACT_LAYOUT_BREAKPOINT_PX);
    };

    syncViewportMode();
    window.addEventListener("resize", syncViewportMode);
    return () => window.removeEventListener("resize", syncViewportMode);
  }, []);

  useEffect(() => {
    const wasCompactLayout = wasCompactLayoutRef.current;
    const enteredCompactLayout =
      wasCompactLayout === null
        ? isCompactLayout
        : !wasCompactLayout && isCompactLayout;
    wasCompactLayoutRef.current = isCompactLayout;

    if (!enteredCompactLayout) {
      return;
    }

    if (sidebarOpen) {
      toggleSidebar();
    }

    if (rightPanelOpen) {
      toggleRightPanel();
    }
  }, [
    isCompactLayout,
    rightPanelOpen,
    sidebarOpen,
    toggleRightPanel,
    toggleSidebar,
  ]);

  const handleCreateWorkspace = () => {
    const token = Date.now().toString(36);
    const now = new Date().toISOString();
    const workspaceId = `ws-${token}`;
    const workspace: Workspace = {
      id: workspaceId,
      slug: workspaceId,
      name: `Workspace ${workspaces.length + 1}`,
      role: "owner",
      createdAt: now,
      updatedAt: now,
      etag: `workspace-${token}`,
    };

    upsertWorkspace(workspace);
    setActiveWorkspaceId(workspace.id);
    toast.success(`Created workspace "${workspace.name}".`);
  };

  const handleDocumentSelect = (documentId: string) => {
    if (!activeWorkspaceId) {
      return;
    }
    setActiveDocumentForWorkspace(activeWorkspaceId, documentId);
    navigate(
      `/workspace/${encodeURIComponent(activeWorkspaceId)}/document/${encodeURIComponent(documentId)}`,
    );
  };

  const handleSearchResultSelect = (documentId: string) => {
    handleDocumentSelect(documentId);
    setSidebarPanel("files");
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

    if (action === "new-folder") {
      createDocumentInNewFolder(document);
      return;
    }
  };

  const handleBacklinkSelect = (documentId: string) => {
    handleDocumentSelect(documentId);
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

  return (
    <div className={styles.layout} data-testid="app-layout">
      {sidebarOpen ? (
        <aside
          aria-label="Sidebar"
          className={styles.sidebar}
          data-testid="app-sidebar"
        >
          <div className={styles.sidebarHeader}>
            <h2 className={styles.sidebarTitle}>Workspace</h2>
            <button
              className={styles.secondaryButton}
              data-testid="sidebar-toggle"
              onClick={toggleSidebar}
              type="button"
            >
              Hide
            </button>
          </div>
          <WorkspaceDropdown
            activeWorkspaceId={activeWorkspaceId}
            onCreateWorkspace={handleCreateWorkspace}
            onWorkspaceSelect={setActiveWorkspaceId}
            workspaces={workspaces}
          />
          <CommandPalette
            activeWorkspaceId={activeWorkspaceId}
            documents={documents}
            onCreateDocument={createUntitledDocument}
            onCreateWorkspace={handleCreateWorkspace}
            onOpenSearchPanel={() => setSidebarPanel("search")}
            onOpenChange={(open) =>
              open ? openCommandPalette() : closeCommandPalette()
            }
            open={commandPaletteOpen}
            openDocumentIds={openDocumentIds}
            workspaces={workspaces}
          />
          <TagsList
            activeTag={activeTag}
            onTagSelect={setActiveTag}
            tags={workspaceTags}
          />
          {searchPanelOpen ? (
            <SearchPanel
              loading={showPanelSkeletons}
              onClose={() => setSidebarPanel("files")}
              onResultSelect={(result) =>
                handleSearchResultSelect(result.documentId)
              }
              results={searchPanelResults}
            />
          ) : (
            <section aria-label="Document tree section">
              <div className={styles.documentTreeHeader}>
                <h2 className={styles.documentTreeHeading}>
                  Documents
                </h2>
                <button
                  className={styles.secondaryButton}
                  data-testid="new-document-button"
                  onClick={createUntitledDocument}
                  title="Cmd+N"
                  type="button"
                >
                  + New
                </button>
              </div>
              <DocumentTree
                activeDocumentId={activeDocumentId}
                documents={filteredDocuments}
                loading={showPanelSkeletons}
                onContextMenuAction={handleDocumentContextAction}
                onDocumentSelect={handleDocumentSelect}
                onRenameDocument={handleRenameDocument}
                pendingRenameDocumentId={pendingRenameDocumentId}
              />
            </section>
          )}
          <AgentsSection peers={remotePeers} />
        </aside>
      ) : (
        <button
          aria-label="Show sidebar"
          className={styles.showSidebarButton}
          data-testid="sidebar-toggle"
          onClick={toggleSidebar}
          type="button"
        >
          Show Sidebar
        </button>
      )}
      <main
        aria-label="Editor area"
        className={styles.editorArea}
        data-testid="app-editor-area"
        ref={(node) => {
          editorAreaRef.current = node;
          setOutlineContainer(node);
        }}
      >
        <Outlet />
      </main>
      {rightPanelOpen ? (
        <aside
          aria-label="Document outline panel"
          className={styles.outlinePanel}
          data-testid="outline-panel"
        >
          <div className={styles.panelHeader}>
            <h2 className={styles.panelHeading}>Outline</h2>
            <button
              className={styles.secondaryButton}
              data-testid="outline-panel-toggle"
              onClick={toggleRightPanel}
              type="button"
            >
              Hide
            </button>
          </div>
          <div
            aria-label="Right panel tabs"
            className={styles.panelTabs}
            role="tablist"
          >
            <button
              aria-pressed={rightPanelTab === "outline"}
              className={
                rightPanelTab === "outline"
                  ? styles.panelTabButtonActive
                  : styles.panelTabButton
              }
              data-testid="right-panel-tab-outline"
              onClick={() => setRightPanelTab("outline")}
              type="button"
            >
              Outline
            </button>
            <button
              aria-pressed={rightPanelTab === "backlinks"}
              className={
                rightPanelTab === "backlinks"
                  ? styles.panelTabButtonActive
                  : styles.panelTabButton
              }
              data-testid="right-panel-tab-backlinks"
              onClick={() => setRightPanelTab("backlinks")}
              type="button"
            >
              Backlinks
            </button>
            <button
              aria-pressed={rightPanelTab === "comments"}
              className={
                rightPanelTab === "comments"
                  ? styles.panelTabButtonActive
                  : styles.panelTabButton
              }
              data-testid="right-panel-tab-comments"
              onClick={() => setRightPanelTab("comments")}
              type="button"
            >
              Comments
            </button>
          </div>

          {rightPanelTab === "outline" ? (
            <Outline
              editorContainer={outlineContainer}
              loading={showOutlineSkeleton}
            />
          ) : null}

          {rightPanelTab === "backlinks" ? (
            <section
              aria-label="Incoming backlinks"
              className={styles.backlinksSection}
              data-testid="backlinks-panel"
            >
              <Backlinks
                backlinks={incomingBacklinkEntries}
                documentId={activeDocumentId ?? ""}
                loading={showPanelSkeletons}
                onBacklinkSelect={handleBacklinkSelect}
                workspaceId={activeWorkspaceId ?? ""}
              />
            </section>
          ) : null}

          {rightPanelTab === "comments" ? (
            <p className={styles.commentsPlaceholder} data-testid="comments-panel-empty">
              Comments panel is coming soon.
            </p>
          ) : null}
        </aside>
      ) : (
        <button
          aria-label="Show document outline panel"
          className={styles.showOutlineButton}
          data-testid="outline-panel-toggle"
          onClick={() => setRightPanelTab("outline")}
          type="button"
        >
          Show Outline
        </button>
      )}
      <AlertDialog.Root
        onOpenChange={(open) => {
          if (!open) {
            handleCancelDeleteDocument();
          }
        }}
        open={Boolean(pendingDeleteDocument)}
      >
        <AlertDialog.Portal>
          <AlertDialog.Backdrop
            className={styles.deleteOverlay}
            data-testid="delete-document-overlay"
          />
          <AlertDialog.Popup
            aria-label="Delete document confirmation"
            className={styles.deleteDialog}
            data-testid="delete-document-dialog"
          >
            <AlertDialog.Title className={styles.deleteDialogTitle}>
              Delete document?
            </AlertDialog.Title>
            <AlertDialog.Description className={styles.deleteDialogDescription}>
              Permanently delete <strong>{pendingDeleteDocument?.path}</strong>?
              This cannot be undone.
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
                onClick={handleConfirmDeleteDocument}
                type="button"
              >
                Delete
              </button>
            </div>
          </AlertDialog.Popup>
        </AlertDialog.Portal>
      </AlertDialog.Root>
      <ToastViewport />
    </div>
  );
}

export const AppLayout = Layout;
