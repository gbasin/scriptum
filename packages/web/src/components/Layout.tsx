import type { Document, Workspace } from "@scriptum/shared";
import { useEffect, useMemo, useRef, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { useDocumentsStore } from "../store/documents";
import { usePresenceStore } from "../store/presence";
import { useWorkspaceStore } from "../store/workspace";
import { CommandPalette } from "./CommandPalette";
import { AgentsSection } from "./sidebar/AgentsSection";
import { DocumentTree, type ContextMenuAction } from "./sidebar/DocumentTree";
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

const OUTLINE_ACTIVE_OFFSET_PX = 120;

interface OutlineHeading {
  id: string;
  level: number;
  text: string;
}

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

function normalizeHeadingSlug(value: string): string {
  const slug = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug.length > 0 ? slug : "section";
}

function collectOutlineHeadings(container: HTMLElement | null): OutlineHeading[] {
  if (!container) {
    return [];
  }

  const headingElements = Array.from(
    container.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  );

  return headingElements.map((heading, index) => {
    const text = heading.textContent?.trim() ?? "";
    if (!heading.id) {
      heading.id = `outline-${normalizeHeadingSlug(text)}-${index + 1}`;
    }

    const headingLevelRaw = Number.parseInt(heading.tagName.slice(1), 10);
    const level = Number.isFinite(headingLevelRaw)
      ? Math.min(6, Math.max(1, headingLevelRaw))
      : 1;

    return {
      id: heading.id,
      level,
      text: text || `Section ${index + 1}`,
    };
  });
}

function detectActiveOutlineHeadingId(
  container: HTMLElement | null,
): string | null {
  if (!container) {
    return null;
  }

  const headingElements = Array.from(
    container.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  );
  if (headingElements.length === 0) {
    return null;
  }

  let activeHeadingId: string | null = null;
  for (const heading of headingElements) {
    if (!heading.id) {
      continue;
    }
    if (heading.getBoundingClientRect().top <= OUTLINE_ACTIVE_OFFSET_PX) {
      activeHeadingId = heading.id;
    }
  }

  if (activeHeadingId) {
    return activeHeadingId;
  }

  return headingElements.find((heading) => heading.id)?.id ?? null;
}

export function Layout() {
  const navigate = useNavigate();
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
  const openDocument = useDocumentsStore((state) => state.openDocument);
  const removeDocument = useDocumentsStore((state) => state.removeDocument);
  const upsertDocument = useDocumentsStore((state) => state.upsertDocument);
  const openDocumentIds = useDocumentsStore((state) => state.openDocumentIds);
  const remotePeers = usePresenceStore((state) => state.remotePeers);
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [pendingRenameDocumentId, setPendingRenameDocumentId] = useState<
    string | null
  >(null);
  const [searchPanelOpen, setSearchPanelOpen] = useState(false);
  const [outlinePanelOpen, setOutlinePanelOpen] = useState(true);
  const [outlineHeadings, setOutlineHeadings] = useState<OutlineHeading[]>([]);
  const [activeOutlineHeadingId, setActiveOutlineHeadingId] = useState<
    string | null
  >(null);
  const editorAreaRef = useRef<HTMLElement | null>(null);

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
    const existingPaths = new Set(workspaceDocuments.map((document) => document.path));
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
      setSearchPanelOpen(true);
    };

    if (typeof window === "undefined") {
      return undefined;
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [createUntitledDocument]);

  useEffect(() => {
    const container = editorAreaRef.current;
    if (!container || typeof window === "undefined") {
      return undefined;
    }

    let latestOutlineHeadings = collectOutlineHeadings(container);
    setOutlineHeadings(latestOutlineHeadings);
    setActiveOutlineHeadingId(detectActiveOutlineHeadingId(container));

    const refreshOutline = () => {
      latestOutlineHeadings = collectOutlineHeadings(container);
      setOutlineHeadings(latestOutlineHeadings);
      setActiveOutlineHeadingId(detectActiveOutlineHeadingId(container));
    };

    const updateActiveHeading = () => {
      if (latestOutlineHeadings.length === 0) {
        setActiveOutlineHeadingId(null);
        return;
      }
      setActiveOutlineHeadingId(detectActiveOutlineHeadingId(container));
    };

    const observer = new MutationObserver(refreshOutline);
    observer.observe(container, {
      characterData: true,
      childList: true,
      subtree: true,
    });

    window.addEventListener("scroll", updateActiveHeading, { passive: true });
    window.addEventListener("resize", updateActiveHeading);

    return () => {
      observer.disconnect();
      window.removeEventListener("scroll", updateActiveHeading);
      window.removeEventListener("resize", updateActiveHeading);
    };
  }, [activeDocumentId, activeWorkspaceId]);

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
    navigate(
      `/workspace/${encodeURIComponent(activeWorkspaceId)}/document/${encodeURIComponent(documentId)}`,
    );
  };

  const handleSearchResultSelect = (documentId: string) => {
    handleDocumentSelect(documentId);
    setSearchPanelOpen(false);
  };

  const handleOutlineHeadingClick = (headingId: string) => {
    const container = editorAreaRef.current;
    if (!container) {
      return;
    }

    const targetHeading = Array.from(
      container.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
    ).find((heading) => heading.id === headingId);
    if (!targetHeading) {
      return;
    }

    targetHeading.scrollIntoView({ behavior: "smooth", block: "start" });
    setActiveOutlineHeadingId(headingId);
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
        <CommandPalette
          activeWorkspaceId={activeWorkspaceId}
          documents={documents}
          onCreateWorkspace={handleCreateWorkspace}
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
        ref={editorAreaRef}
        style={{ flex: 1, padding: "1rem" }}
      >
        <Outlet />
      </main>
      {outlinePanelOpen ? (
        <aside
          aria-label="Document outline panel"
          data-testid="outline-panel"
          style={{
            borderLeft: "1px solid #d1d5db",
            padding: "1rem",
            width: "16rem",
          }}
        >
          <div
            style={{
              alignItems: "center",
              display: "flex",
              justifyContent: "space-between",
              marginBottom: "0.75rem",
            }}
          >
            <h2 style={{ margin: 0 }}>Outline</h2>
            <button
              data-testid="outline-panel-toggle"
              onClick={() => setOutlinePanelOpen(false)}
              type="button"
            >
              Hide
            </button>
          </div>

          {outlineHeadings.length === 0 ? (
            <p data-testid="outline-empty" style={{ color: "#6b7280", margin: 0 }}>
              No headings in this document.
            </p>
          ) : (
            <ul
              aria-label="Document heading outline"
              data-testid="outline-list"
              style={{ listStyle: "none", margin: 0, padding: 0 }}
            >
              {outlineHeadings.map((heading) => (
                <li key={heading.id}>
                  <button
                    data-active={heading.id === activeOutlineHeadingId}
                    data-testid={`outline-heading-${heading.id}`}
                    onClick={() => handleOutlineHeadingClick(heading.id)}
                    style={{
                      background:
                        heading.id === activeOutlineHeadingId ? "#e0e7ff" : "transparent",
                      border: "none",
                      borderRadius: "0.375rem",
                      color: "#111827",
                      cursor: "pointer",
                      display: "block",
                      fontSize: "0.875rem",
                      marginBottom: "0.25rem",
                      padding: "0.3rem 0.4rem",
                      paddingLeft: `${0.4 + (heading.level - 1) * 0.6}rem`,
                      textAlign: "left",
                      width: "100%",
                    }}
                    type="button"
                  >
                    {heading.text}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>
      ) : (
        <button
          aria-label="Show document outline panel"
          data-testid="outline-panel-toggle"
          onClick={() => setOutlinePanelOpen(true)}
          style={{
            alignSelf: "flex-start",
            background: "#ffffff",
            border: "1px solid #d1d5db",
            borderRadius: "0.375rem",
            cursor: "pointer",
            margin: "1rem 1rem 0 0",
            padding: "0.4rem 0.6rem",
          }}
          type="button"
        >
          Show Outline
        </button>
      )}
    </div>
  );
}

export const AppLayout = Layout;
