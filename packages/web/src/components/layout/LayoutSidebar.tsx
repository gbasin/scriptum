import type { Document, Workspace } from "@scriptum/shared";
import clsx from "clsx";
import type { PeerPresence } from "../../store/presence";
import type { SidebarPanel } from "../../store/ui";
import { AgentsSection } from "../sidebar/AgentsSection";
import {
  type ContextMenuAction,
  DocumentTree,
} from "../sidebar/DocumentTree";
import {
  type SearchPanelResult,
  SearchPanel,
} from "../sidebar/SearchPanel";
import { CommandPalette } from "../CommandPalette";
import styles from "../Layout.module.css";
import { TagsList } from "../sidebar/TagsList";
import { WorkspaceDropdown } from "../sidebar/WorkspaceDropdown";

export interface LayoutSidebarProps {
  activeDocumentId: string | null;
  activeTag: string | null;
  activeWorkspaceId: string | null;
  archivedWorkspaceCount: number;
  commandPaletteOpen: boolean;
  documents: readonly Document[];
  openDocumentIds: readonly string[];
  pendingRenameDocumentId: string | null;
  remotePeers: readonly PeerPresence[];
  searchPanelOpen: boolean;
  searchPanelResults: readonly SearchPanelResult[];
  showArchivedDocuments: boolean;
  showPanelSkeletons: boolean;
  sidebarOpen: boolean;
  visibleDocuments: readonly Document[];
  workspaceTags: readonly string[];
  workspaces: readonly Workspace[];
  onCommandPaletteOpenChange: (open: boolean) => void;
  onCreateDocument: () => void;
  onCreateWorkspace: () => void;
  onDocumentContextAction: (action: ContextMenuAction, document: Document) => void;
  onDocumentSelect: (documentId: string) => void;
  onRenameDocument: (documentId: string, nextPath: string) => void;
  onSearchResultSelect: (documentId: string) => void;
  onSidebarPanelChange: (panel: SidebarPanel) => void;
  onTagSelect: (tag: string | null) => void;
  onToggleArchivedDocuments: () => void;
  onToggleSidebar: () => void;
  onWorkspaceSelect: (workspaceId: string) => void;
}

export function LayoutSidebar({
  activeDocumentId,
  activeTag,
  activeWorkspaceId,
  archivedWorkspaceCount,
  commandPaletteOpen,
  documents,
  openDocumentIds,
  pendingRenameDocumentId,
  remotePeers,
  searchPanelOpen,
  searchPanelResults,
  showArchivedDocuments,
  showPanelSkeletons,
  sidebarOpen,
  visibleDocuments,
  workspaceTags,
  workspaces,
  onCommandPaletteOpenChange,
  onCreateDocument,
  onCreateWorkspace,
  onDocumentContextAction,
  onDocumentSelect,
  onRenameDocument,
  onSearchResultSelect,
  onSidebarPanelChange,
  onTagSelect,
  onToggleArchivedDocuments,
  onToggleSidebar,
  onWorkspaceSelect,
}: LayoutSidebarProps) {
  if (!sidebarOpen) {
    return (
      <button
        aria-label="Show sidebar"
        className={styles.showSidebarButton}
        data-testid="sidebar-toggle"
        onClick={onToggleSidebar}
        type="button"
      >
        Show Sidebar
      </button>
    );
  }

  return (
    <aside aria-label="Sidebar" className={styles.sidebar} data-testid="app-sidebar">
      <div className={styles.sidebarHeader}>
        <h2 className={styles.sidebarTitle}>Workspace</h2>
        <button
          className={styles.secondaryButton}
          data-testid="sidebar-toggle"
          onClick={onToggleSidebar}
          type="button"
        >
          Hide
        </button>
      </div>
      <WorkspaceDropdown
        activeWorkspaceId={activeWorkspaceId}
        onCreateWorkspace={onCreateWorkspace}
        onWorkspaceSelect={onWorkspaceSelect}
        workspaces={workspaces.slice()}
      />
      <CommandPalette
        activeWorkspaceId={activeWorkspaceId}
        documents={documents.slice()}
        onCreateDocument={onCreateDocument}
        onCreateWorkspace={onCreateWorkspace}
        onOpenSearchPanel={() => onSidebarPanelChange("search")}
        onOpenChange={onCommandPaletteOpenChange}
        open={commandPaletteOpen}
        openDocumentIds={openDocumentIds.slice()}
        workspaces={workspaces.slice()}
      />
      <TagsList activeTag={activeTag} onTagSelect={onTagSelect} tags={workspaceTags} />
      {searchPanelOpen ? (
        <SearchPanel
          loading={showPanelSkeletons}
          onClose={() => onSidebarPanelChange("files")}
          onResultSelect={(result) => onSearchResultSelect(result.documentId)}
          results={searchPanelResults}
        />
      ) : (
        <section aria-label="Document tree section">
          <div className={styles.documentTreeHeader}>
            <h2 className={styles.documentTreeHeading}>
              {showArchivedDocuments ? "Archived" : "Documents"}
            </h2>
            <div className={styles.documentTreeActions}>
              <button
                aria-pressed={showArchivedDocuments}
                className={clsx(
                  styles.secondaryButton,
                  showArchivedDocuments && styles.secondaryButtonActive,
                )}
                data-testid="document-tree-archive-toggle"
                disabled={!showArchivedDocuments && archivedWorkspaceCount === 0}
                onClick={onToggleArchivedDocuments}
                type="button"
              >
                {showArchivedDocuments
                  ? "Show active"
                  : `Archived (${archivedWorkspaceCount})`}
              </button>
              <button
                className={styles.secondaryButton}
                data-testid="new-document-button"
                onClick={onCreateDocument}
                title="Cmd+N"
                type="button"
              >
                + New
              </button>
            </div>
          </div>
          <DocumentTree
            activeDocumentId={activeDocumentId}
            documents={visibleDocuments.slice()}
            loading={showPanelSkeletons}
            onContextMenuAction={onDocumentContextAction}
            onDocumentSelect={onDocumentSelect}
            onRenameDocument={onRenameDocument}
            pendingRenameDocumentId={pendingRenameDocumentId}
          />
        </section>
      )}
      <AgentsSection peers={remotePeers.slice()} />
    </aside>
  );
}
