import type { Document, Workspace } from "@scriptum/shared";
import { useEffect, useMemo, useRef, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { useDocumentCrud } from "../hooks/useDocumentCrud";
import { useLayoutState } from "../hooks/useLayoutState";
import { useToast } from "../hooks/useToast";
import {
  buildIncomingBacklinks,
  type IncomingBacklink,
} from "../lib/wiki-links";
import { CommandPalette } from "./CommandPalette";
import { DeleteDocumentDialog } from "./DeleteDocumentDialog";
import { ErrorBoundary } from "./ErrorBoundary";
import styles from "./Layout.module.css";
import { Backlinks } from "./right-panel/Backlinks";
import { Outline } from "./right-panel/Outline";
import { AgentsSection } from "./sidebar/AgentsSection";
import { DocumentTree } from "./sidebar/DocumentTree";
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
import { ToastViewport } from "./ToastViewport";
export type { IncomingBacklink };

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
const RIGHT_PANEL_TAB_IDS = {
  backlinks: "right-panel-tab-backlinks",
  comments: "right-panel-tab-comments",
  outline: "right-panel-tab-outline",
} as const;
const RIGHT_PANEL_TAB_PANEL_IDS = {
  backlinks: "right-panel-tabpanel-backlinks",
  comments: "right-panel-tabpanel-comments",
  outline: "right-panel-tabpanel-outline",
} as const;

export function Layout() {
  const navigate = useNavigate();
  const {
    activeDocumentIdByWorkspace,
    activeWorkspaceId,
    closeCommandPalette,
    commandPaletteOpen,
    documents,
    openCommandPalette,
    openDocument,
    openDocumentIds,
    removeDocument,
    remotePeers,
    rightPanelOpen,
    rightPanelTab,
    setActiveDocumentForWorkspace,
    setActiveWorkspaceId,
    setRightPanelTab,
    setSidebarPanel,
    sidebarOpen,
    sidebarPanel,
    toggleRightPanel,
    toggleSidebar,
    upsertDocument,
    upsertWorkspace,
    workspaces,
  } = useLayoutState();
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
  const {
    createUntitledDocument,
    handleCancelDeleteDocument,
    handleConfirmDeleteDocument,
    handleDocumentContextAction,
    handleRenameDocument,
  } = useDocumentCrud({
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
    titleFromPath,
    toast,
    upsertDocument,
    workspaceDocuments,
  });

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

  const handleBacklinkSelect = (documentId: string) => {
    handleDocumentSelect(documentId);
  };
  const showCompactPanelBackdrop =
    isCompactLayout && (sidebarOpen || rightPanelOpen);
  const handleCompactPanelBackdropClick = () => {
    if (sidebarOpen) {
      toggleSidebar();
    }
    if (rightPanelOpen) {
      toggleRightPanel();
    }
  };

  return (
    <div className={styles.layout} data-testid="app-layout">
      {showCompactPanelBackdrop ? (
        <button
          aria-label="Close panels"
          className={styles.compactPanelBackdrop}
          data-testid="compact-panel-backdrop"
          onClick={handleCompactPanelBackdropClick}
          type="button"
        />
      ) : null}
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
                <h2 className={styles.documentTreeHeading}>Documents</h2>
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
        <ErrorBoundary
          inline
          message="This view crashed. Reload to recover while keeping navigation available."
          reloadLabel="Reload view"
          testId="route-error-boundary"
          title="View failed to render"
        >
          <Outlet />
        </ErrorBoundary>
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
              aria-controls={RIGHT_PANEL_TAB_PANEL_IDS.outline}
              aria-selected={rightPanelTab === "outline"}
              className={
                rightPanelTab === "outline"
                  ? styles.panelTabButtonActive
                  : styles.panelTabButton
              }
              data-testid={RIGHT_PANEL_TAB_IDS.outline}
              id={RIGHT_PANEL_TAB_IDS.outline}
              onClick={() => setRightPanelTab("outline")}
              role="tab"
              tabIndex={rightPanelTab === "outline" ? 0 : -1}
              type="button"
            >
              Outline
            </button>
            <button
              aria-controls={RIGHT_PANEL_TAB_PANEL_IDS.backlinks}
              aria-selected={rightPanelTab === "backlinks"}
              className={
                rightPanelTab === "backlinks"
                  ? styles.panelTabButtonActive
                  : styles.panelTabButton
              }
              data-testid={RIGHT_PANEL_TAB_IDS.backlinks}
              id={RIGHT_PANEL_TAB_IDS.backlinks}
              onClick={() => setRightPanelTab("backlinks")}
              role="tab"
              tabIndex={rightPanelTab === "backlinks" ? 0 : -1}
              type="button"
            >
              Backlinks
            </button>
            <button
              aria-controls={RIGHT_PANEL_TAB_PANEL_IDS.comments}
              aria-selected={rightPanelTab === "comments"}
              className={
                rightPanelTab === "comments"
                  ? styles.panelTabButtonActive
                  : styles.panelTabButton
              }
              data-testid={RIGHT_PANEL_TAB_IDS.comments}
              id={RIGHT_PANEL_TAB_IDS.comments}
              onClick={() => setRightPanelTab("comments")}
              role="tab"
              tabIndex={rightPanelTab === "comments" ? 0 : -1}
              type="button"
            >
              Comments
            </button>
          </div>

          {rightPanelTab === "outline" ? (
            <section
              aria-labelledby={RIGHT_PANEL_TAB_IDS.outline}
              data-testid="right-panel-tabpanel-outline"
              id={RIGHT_PANEL_TAB_PANEL_IDS.outline}
              role="tabpanel"
            >
              <Outline
                editorContainer={outlineContainer}
                loading={showOutlineSkeleton}
              />
            </section>
          ) : null}

          {rightPanelTab === "backlinks" ? (
            <section
              aria-label="Incoming backlinks"
              aria-labelledby={RIGHT_PANEL_TAB_IDS.backlinks}
              className={styles.backlinksSection}
              data-testid="backlinks-panel"
              id={RIGHT_PANEL_TAB_PANEL_IDS.backlinks}
              role="tabpanel"
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
            <section
              aria-labelledby={RIGHT_PANEL_TAB_IDS.comments}
              data-testid="right-panel-tabpanel-comments"
              id={RIGHT_PANEL_TAB_PANEL_IDS.comments}
              role="tabpanel"
            >
              <p
                className={styles.commentsPlaceholder}
                data-testid="comments-panel-empty"
              >
                Comments panel is coming soon.
              </p>
            </section>
          ) : null}
        </aside>
      ) : (
        <button
          aria-label="Show document outline panel"
          className={styles.showOutlineButton}
          data-testid="outline-panel-toggle"
          onClick={toggleRightPanel}
          type="button"
        >
          Show Outline
        </button>
      )}
      <DeleteDocumentDialog
        documentPath={pendingDeleteDocument?.path ?? null}
        onCancel={handleCancelDeleteDocument}
        onConfirm={handleConfirmDeleteDocument}
        open={Boolean(pendingDeleteDocument)}
      />
      <ToastViewport />
    </div>
  );
}

export const AppLayout = Layout;
