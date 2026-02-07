import type { Workspace } from "@scriptum/shared";
import { useEffect, useMemo, useState } from "react";
import { Outlet } from "react-router-dom";
import { useDocumentsStore } from "../store/documents";
import { usePresenceStore } from "../store/presence";
import { useWorkspaceStore } from "../store/workspace";
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

export function Layout() {
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId);
  const setActiveWorkspaceId = useWorkspaceStore(
    (state) => state.setActiveWorkspaceId
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
  const remotePeers = usePresenceStore((state) => state.remotePeers);
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [searchPanelOpen, setSearchPanelOpen] = useState(false);

  const workspaceDocuments = useMemo(
    () =>
      activeWorkspaceId
        ? documents.filter((document) => document.workspaceId === activeWorkspaceId)
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
    ? activeDocumentIdByWorkspace[activeWorkspaceId] ?? null
    : null;

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
      if (!isSearchPanelShortcut(event)) {
        return;
      }
      event.preventDefault();
      setSearchPanelOpen(true);
    };

    if (typeof window === "undefined") {
      return undefined;
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

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
  };

  const handleDocumentSelect = (documentId: string) => {
    if (!activeWorkspaceId) {
      return;
    }
    setActiveDocumentForWorkspace(activeWorkspaceId, documentId);
  };

  const handleSearchResultSelect = (documentId: string) => {
    handleDocumentSelect(documentId);
    setSearchPanelOpen(false);
  };

  return (
    <div
      data-testid="app-layout"
      style={{ display: "flex", minHeight: "100vh" }}
    >
      <aside
        aria-label="Sidebar"
        data-testid="app-sidebar"
        style={{
          borderRight: "1px solid #d1d5db",
          padding: "1rem",
          width: "18rem",
        }}
      >
        <WorkspaceDropdown
          activeWorkspaceId={activeWorkspaceId}
          onCreateWorkspace={handleCreateWorkspace}
          onWorkspaceSelect={setActiveWorkspaceId}
          workspaces={workspaces}
        />
        <TagsList
          activeTag={activeTag}
          onTagSelect={setActiveTag}
          tags={workspaceTags}
        />
        {searchPanelOpen ? (
          <SearchPanel
            onClose={() => setSearchPanelOpen(false)}
            onResultSelect={(result) => handleSearchResultSelect(result.documentId)}
            results={searchPanelResults}
          />
        ) : (
          <section aria-label="Document tree section">
            <h2 style={{ marginBottom: "0.25rem", marginTop: "1rem" }}>Documents</h2>
            <DocumentTree
              activeDocumentId={activeDocumentId}
              documents={filteredDocuments}
              onDocumentSelect={handleDocumentSelect}
            />
          </section>
        )}
        <AgentsSection peers={remotePeers} />
      </aside>
      <main
        aria-label="Editor area"
        data-testid="app-editor-area"
        style={{ flex: 1, padding: "1rem" }}
      >
        <Outlet />
      </main>
    </div>
  );
}

export const AppLayout = Layout;
