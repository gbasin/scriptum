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

interface ParsedWikiLink {
  raw: string;
  target: string;
}

export interface IncomingBacklink {
  sourceDocumentId: string;
  sourcePath: string;
  sourceTitle: string;
  snippet: string;
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

function normalizeBacklinkTarget(value: string): string {
  return value.trim().toLowerCase();
}

function baseName(path: string): string {
  const segments = path.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

function baseNameWithoutExtension(path: string): string {
  return baseName(path).replace(/\.[^.]+$/, "");
}

function extractWikiLinks(markdown: string): ParsedWikiLink[] {
  const links: ParsedWikiLink[] = [];
  const pattern = /\[\[([^[\]]+)\]\]/g;
  let match: RegExpExecArray | null = pattern.exec(markdown);

  while (match) {
    const rawInner = match[1]?.trim() ?? "";
    if (rawInner.length > 0) {
      const [targetWithHeading] = rawInner.split("|");
      const [target] = targetWithHeading.split("#");
      const normalizedTarget = normalizeBacklinkTarget(target);
      if (normalizedTarget.length > 0) {
        links.push({
          raw: match[0],
          target: normalizedTarget,
        });
      }
    }
    match = pattern.exec(markdown);
  }

  return links;
}

function targetAliases(document: Document): Set<string> {
  const aliases = new Set<string>();
  const pathNormalized = normalizeBacklinkTarget(document.path);
  const pathBaseName = normalizeBacklinkTarget(baseName(document.path));
  const pathBaseNameWithoutExtension = normalizeBacklinkTarget(
    baseNameWithoutExtension(document.path),
  );
  const titleNormalized = normalizeBacklinkTarget(document.title);

  if (pathNormalized.length > 0) {
    aliases.add(pathNormalized);
  }
  if (pathBaseName.length > 0) {
    aliases.add(pathBaseName);
  }
  if (pathBaseNameWithoutExtension.length > 0) {
    aliases.add(pathBaseNameWithoutExtension);
  }
  if (titleNormalized.length > 0) {
    aliases.add(titleNormalized);
  }

  return aliases;
}

export function buildIncomingBacklinks(
  documents: readonly Document[],
  activeDocumentId: string | null,
): IncomingBacklink[] {
  if (!activeDocumentId) {
    return [];
  }

  const activeDocument = documents.find((document) => document.id === activeDocumentId);
  if (!activeDocument) {
    return [];
  }
  const aliases = targetAliases(activeDocument);
  const backlinks: IncomingBacklink[] = [];

  for (const document of documents) {
    if (document.id === activeDocument.id || typeof document.bodyMd !== "string") {
      continue;
    }
    const link = extractWikiLinks(document.bodyMd).find((candidate) =>
      aliases.has(candidate.target),
    );
    if (!link) {
      continue;
    }

    backlinks.push({
      sourceDocumentId: document.id,
      sourcePath: document.path,
      sourceTitle: document.title,
      snippet: link.raw,
    });
  }

  return backlinks.sort((left, right) =>
    left.sourcePath.localeCompare(right.sourcePath),
  );
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
  const incomingBacklinks = useMemo(
    () => buildIncomingBacklinks(workspaceDocuments, activeDocumentId),
    [workspaceDocuments, activeDocumentId],
  );

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

  const updateExistingDocument = (
    documentId: string,
    updater: (document: Document) => Document,
  ) => {
    const currentDocument = documents.find((document) => document.id === documentId);
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
    const now = new Date().toISOString();
    updateExistingDocument(documentId, (document) => ({
      ...document,
      etag: `${document.etag}:rename:${Date.now().toString(36)}`,
      path: normalizedPath,
      title: titleFromPath(normalizedPath),
      updatedAt: now,
    }));
    setPendingRenameDocumentId(null);
  };

  const createDocumentInNewFolder = (sourceDocument: Document) => {
    const segments = sourceDocument.path.split("/").filter(Boolean);
    const parentPath = segments.slice(0, -1).join("/");
    const existingPaths = new Set(workspaceDocuments.map((document) => document.path));
    let suffix = 1;
    let folderName = "new-folder";
    let candidatePath = `${folderName}/untitled.md`;

    while (
      existingPaths.has(
        parentPath.length > 0 ? `${parentPath}/${candidatePath}` : candidatePath,
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
      removeDocument(document.id);
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
        void navigator.clipboard.writeText(link);
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

  const handleBacklinkSelect = (documentId: string) => {
    handleDocumentSelect(documentId);
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
              onContextMenuAction={handleDocumentContextAction}
              onDocumentSelect={handleDocumentSelect}
              onRenameDocument={handleRenameDocument}
              pendingRenameDocumentId={pendingRenameDocumentId}
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

          <section
            aria-label="Incoming backlinks"
            data-testid="backlinks-panel"
            style={{ marginTop: "1.25rem" }}
          >
            <h3 style={{ margin: "0 0 0.5rem" }}>Backlinks</h3>
            {incomingBacklinks.length === 0 ? (
              <p data-testid="backlinks-empty" style={{ color: "#6b7280", margin: 0 }}>
                No incoming links to this document.
              </p>
            ) : (
              <ul
                aria-label="Incoming wiki links"
                data-testid="backlinks-list"
                style={{ listStyle: "none", margin: 0, padding: 0 }}
              >
                {incomingBacklinks.map((backlink) => (
                  <li key={backlink.sourceDocumentId} style={{ marginBottom: "0.75rem" }}>
                    <button
                      data-testid={`backlink-item-${backlink.sourceDocumentId}`}
                      onClick={() => handleBacklinkSelect(backlink.sourceDocumentId)}
                      style={{
                        background: "transparent",
                        border: "none",
                        color: "#1d4ed8",
                        cursor: "pointer",
                        fontSize: "0.875rem",
                        fontWeight: 600,
                        padding: 0,
                        textAlign: "left",
                      }}
                      type="button"
                    >
                      {backlink.sourceTitle}
                    </button>
                    <p
                      data-testid={`backlink-snippet-${backlink.sourceDocumentId}`}
                      style={{ color: "#6b7280", fontSize: "0.8rem", margin: "0.2rem 0 0" }}
                    >
                      {backlink.snippet}
                    </p>
                  </li>
                ))}
              </ul>
            )}
          </section>
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
