import type { Document, Workspace } from "@scriptum/shared";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  baseName,
  COMPACT_LAYOUT_BREAKPOINT_PX,
  formatRenameBacklinkToast,
  isNewDocumentShortcut,
  normalizeTag,
  parentFolderPath,
} from "../components/layout/layoutUtils";
import type { BacklinkEntry } from "../components/right-panel/Backlinks";
import {
  buildSearchPanelResults,
  isSearchPanelShortcut,
} from "../components/sidebar/SearchPanel";
import {
  collectWorkspaceTags,
  filterDocumentsByTag,
} from "../components/sidebar/TagsList";
import { useDocumentCrud } from "../hooks/useDocumentCrud";
import { useLayoutState } from "../hooks/useLayoutState";
import { useToast } from "../hooks/useToast";
import {
  buildIncomingBacklinks,
  type IncomingBacklink,
} from "../lib/wiki-links";
import { useCommentsStore } from "../store/comments";

export interface LayoutController {
  dialogs: {
    onCancelAddTag: () => void;
    onCancelDeleteDocument: () => void;
    onCancelMoveDocument: () => void;
    onConfirmAddTag: () => void;
    onConfirmDeleteDocument: () => void;
    onConfirmMoveDocument: () => void;
    onMoveDestinationChange: (path: string) => void;
    onTagChange: (value: string) => void;
    pendingDeleteDocument: Document | null;
    pendingMoveDestination: string | null;
    pendingMoveDocument: Document | null;
    pendingTagDocument: Document | null;
    pendingTagValue: string;
    workspaceDocuments: Document[];
    workspaceTags: string[];
  };
  handleCompactPanelBackdropClick: () => void;
  handleEditorAreaRef: (node: HTMLElement | null) => void;
  rightPanel: {
    activeDocumentId: string | null;
    activeWorkspaceId: string | null;
    incomingBacklinkEntries: BacklinkEntry[];
    onBacklinkSelect: (documentId: string) => void;
    onCommentThreadSelect: (documentId: string, threadId: string) => void;
    onRightPanelTabChange: (tab: "outline" | "backlinks" | "comments") => void;
    onToggleRightPanel: () => void;
    outlineContainer: HTMLElement | null;
    rightPanelOpen: boolean;
    rightPanelTab: "outline" | "backlinks" | "comments";
    showOutlineSkeleton: boolean;
    showPanelSkeletons: boolean;
    threadsByDocumentKey: ReturnType<
      typeof useCommentsStore.getState
    >["threadsByDocumentKey"];
    workspaceDocuments: Document[];
  };
  showCompactPanelBackdrop: boolean;
  sidebar: {
    activeDocumentId: string | null;
    activeTag: string | null;
    activeWorkspaceId: string | null;
    archivedWorkspaceCount: number;
    commandPaletteOpen: boolean;
    documents: Document[];
    onCommandPaletteOpenChange: (open: boolean) => void;
    onCreateDocument: () => void;
    onCreateWorkspace: () => void;
    onDocumentContextAction: (
      action:
        | "new-folder"
        | "rename"
        | "move"
        | "delete"
        | "copy-link"
        | "add-tag"
        | "archive"
        | "unarchive",
      document: Document,
    ) => void;
    onDocumentSelect: (documentId: string) => void;
    onRenameDocument: (documentId: string, nextPath: string) => void;
    onSearchResultSelect: (documentId: string) => void;
    onSidebarPanelChange: (panel: "files" | "search" | "tags") => void;
    onTagSelect: (tag: string | null) => void;
    onToggleArchivedDocuments: () => void;
    onToggleSidebar: () => void;
    onWorkspaceSelect: (workspaceId: string) => void;
    openDocumentIds: string[];
    pendingRenameDocumentId: string | null;
    remotePeers: ReturnType<typeof useLayoutState>["remotePeers"];
    searchPanelOpen: boolean;
    searchPanelResults: ReturnType<typeof buildSearchPanelResults>;
    showArchivedDocuments: boolean;
    showPanelSkeletons: boolean;
    sidebarOpen: boolean;
    visibleDocuments: Document[];
    workspaceTags: string[];
    workspaces: Workspace[];
  };
}

export function useLayoutController(): LayoutController {
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
  const threadsByDocumentKey = useCommentsStore(
    (state) => state.threadsByDocumentKey,
  );
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [showArchivedDocuments, setShowArchivedDocuments] = useState(false);
  const [pendingRenameDocumentId, setPendingRenameDocumentId] = useState<
    string | null
  >(null);
  const [pendingDeleteDocument, setPendingDeleteDocument] =
    useState<Document | null>(null);
  const [pendingMoveDocument, setPendingMoveDocument] =
    useState<Document | null>(null);
  const [pendingMoveDestination, setPendingMoveDestination] = useState<
    string | null
  >(null);
  const [pendingTagDocument, setPendingTagDocument] = useState<Document | null>(
    null,
  );
  const [pendingTagValue, setPendingTagValue] = useState("");
  const [outlineContainer, setOutlineContainer] = useState<HTMLElement | null>(
    null,
  );
  const [isCompactLayout, setIsCompactLayout] = useState(false);
  const wasCompactLayoutRef = useRef<boolean | null>(null);

  const workspaceDocuments = useMemo(
    () =>
      activeWorkspaceId
        ? documents.filter(
            (document) =>
              document.workspaceId === activeWorkspaceId &&
              document.deletedAt === null,
          )
        : [],
    [activeWorkspaceId, documents],
  );
  const activeWorkspaceDocuments = useMemo(
    () => workspaceDocuments.filter((document) => document.archivedAt === null),
    [workspaceDocuments],
  );
  const archivedWorkspaceDocuments = useMemo(
    () => workspaceDocuments.filter((document) => document.archivedAt !== null),
    [workspaceDocuments],
  );
  const workspaceTags = useMemo(
    () => collectWorkspaceTags(workspaceDocuments),
    [workspaceDocuments],
  );
  const filteredActiveDocuments = useMemo(
    () => filterDocumentsByTag(activeWorkspaceDocuments, activeTag),
    [activeWorkspaceDocuments, activeTag],
  );
  const filteredArchivedDocuments = useMemo(
    () => filterDocumentsByTag(archivedWorkspaceDocuments, activeTag),
    [archivedWorkspaceDocuments, activeTag],
  );
  const visibleDocuments = useMemo(
    () =>
      showArchivedDocuments
        ? filteredArchivedDocuments
        : filteredActiveDocuments,
    [filteredActiveDocuments, filteredArchivedDocuments, showArchivedDocuments],
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
    () => mapIncomingBacklinks(incomingBacklinks),
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
    openMoveDialog: (document) => {
      setPendingMoveDocument(document);
      setPendingMoveDestination(parentFolderPath(document.path));
    },
    openTagDialog: (document) => {
      setPendingTagDocument(document);
      setPendingTagValue("");
    },
    pendingDeleteDocument,
    removeDocument,
    setActiveDocumentForWorkspace,
    setPendingDeleteDocument,
    setPendingRenameDocumentId,
    toast,
    upsertDocument,
    workspaceDocuments,
  });

  useEffect(() => {
    setActiveTag(null);
    setShowArchivedDocuments(false);
    setPendingMoveDocument(null);
    setPendingMoveDestination(null);
    setPendingTagDocument(null);
    setPendingTagValue("");
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

  const handleCommentThreadSelect = (documentId: string, threadId: string) => {
    if (!activeWorkspaceId) {
      return;
    }
    setActiveDocumentForWorkspace(activeWorkspaceId, documentId);
    navigate(
      `/workspace/${encodeURIComponent(activeWorkspaceId)}/document/${encodeURIComponent(documentId)}#comment-thread-${encodeURIComponent(threadId)}`,
    );
  };

  const handleCreateNewDocument = () => {
    setShowArchivedDocuments(false);
    createUntitledDocument();
  };

  const handleCancelAddTag = () => {
    setPendingTagDocument(null);
    setPendingTagValue("");
  };

  const handleCancelMoveDocument = () => {
    setPendingMoveDocument(null);
    setPendingMoveDestination(null);
  };

  const handleConfirmMoveDocument = () => {
    const moveDocument = pendingMoveDocument;
    if (!moveDocument || pendingMoveDestination === null) {
      return;
    }

    const currentDocument = documents.find(
      (document) => document.id === moveDocument.id,
    );
    if (!currentDocument) {
      handleCancelMoveDocument();
      return;
    }

    const fileName = baseName(currentDocument.path);
    const nextPath =
      pendingMoveDestination.length > 0
        ? `${pendingMoveDestination}/${fileName}`
        : fileName;

    if (nextPath !== currentDocument.path) {
      handleRenameDocument(currentDocument.id, nextPath);
    }

    handleCancelMoveDocument();
  };

  const handleConfirmAddTag = () => {
    const targetDocument = pendingTagDocument;
    if (!targetDocument) {
      return;
    }

    const normalizedTag = normalizeTag(pendingTagValue);
    if (normalizedTag.length === 0) {
      toast.error("Enter a tag name.");
      return;
    }

    const currentDocument = documents.find(
      (document) => document.id === targetDocument.id,
    );
    if (!currentDocument) {
      handleCancelAddTag();
      return;
    }

    const hasTag = currentDocument.tags.some(
      (tag) => normalizeTag(tag) === normalizedTag,
    );
    if (hasTag) {
      toast.success(`"${normalizedTag}" is already on this document.`);
      handleCancelAddTag();
      return;
    }

    const now = new Date().toISOString();
    upsertDocument({
      ...currentDocument,
      etag: `${currentDocument.etag}:tag:${Date.now().toString(36)}`,
      tags: [...currentDocument.tags, normalizedTag],
      updatedAt: now,
    });
    toast.success(`Added #${normalizedTag} to "${currentDocument.path}".`);
    handleCancelAddTag();
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

  const handleEditorAreaRef = (node: HTMLElement | null) => {
    setOutlineContainer(node);
  };

  return {
    dialogs: {
      onCancelAddTag: handleCancelAddTag,
      onCancelDeleteDocument: handleCancelDeleteDocument,
      onCancelMoveDocument: handleCancelMoveDocument,
      onConfirmAddTag: handleConfirmAddTag,
      onConfirmDeleteDocument: handleConfirmDeleteDocument,
      onConfirmMoveDocument: handleConfirmMoveDocument,
      onMoveDestinationChange: setPendingMoveDestination,
      onTagChange: setPendingTagValue,
      pendingDeleteDocument,
      pendingMoveDestination,
      pendingMoveDocument,
      pendingTagDocument,
      pendingTagValue,
      workspaceDocuments,
      workspaceTags,
    },
    handleCompactPanelBackdropClick,
    handleEditorAreaRef,
    rightPanel: {
      activeDocumentId,
      activeWorkspaceId,
      incomingBacklinkEntries,
      onBacklinkSelect: handleBacklinkSelect,
      onCommentThreadSelect: handleCommentThreadSelect,
      onRightPanelTabChange: setRightPanelTab,
      onToggleRightPanel: toggleRightPanel,
      outlineContainer,
      rightPanelOpen,
      rightPanelTab,
      showOutlineSkeleton,
      showPanelSkeletons,
      threadsByDocumentKey,
      workspaceDocuments,
    },
    showCompactPanelBackdrop,
    sidebar: {
      activeDocumentId,
      activeTag,
      activeWorkspaceId,
      archivedWorkspaceCount: archivedWorkspaceDocuments.length,
      commandPaletteOpen,
      documents,
      onCommandPaletteOpenChange: (open) =>
        open ? openCommandPalette() : closeCommandPalette(),
      onCreateDocument: handleCreateNewDocument,
      onCreateWorkspace: handleCreateWorkspace,
      onDocumentContextAction: handleDocumentContextAction,
      onDocumentSelect: handleDocumentSelect,
      onRenameDocument: handleRenameDocument,
      onSearchResultSelect: handleSearchResultSelect,
      onSidebarPanelChange: setSidebarPanel,
      onTagSelect: setActiveTag,
      onToggleArchivedDocuments: () =>
        setShowArchivedDocuments((current) => !current),
      onToggleSidebar: toggleSidebar,
      onWorkspaceSelect: setActiveWorkspaceId,
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
    },
  };
}

function mapIncomingBacklinks(
  backlinks: readonly IncomingBacklink[],
): BacklinkEntry[] {
  return backlinks.map((backlink) => ({
    docId: backlink.sourceDocumentId,
    path: backlink.sourcePath,
    title: backlink.sourceTitle,
    linkText: backlink.snippet,
    snippet: backlink.snippet,
  }));
}
