import { useDocumentsStore } from "../store/documents";
import { usePresenceStore } from "../store/presence";
import { useUiStore } from "../store/ui";
import { useWorkspaceStore } from "../store/workspace";

export function useLayoutState() {
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

  return {
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
  };
}
