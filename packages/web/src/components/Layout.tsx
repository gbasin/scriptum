import type { Document, Workspace } from "@scriptum/shared";
import { useEffect, useMemo, useRef, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";
import { useDocumentsStore } from "../store/documents";
import { usePresenceStore } from "../store/presence";
import { useWorkspaceStore } from "../store/workspace";
import { CommandPalette } from "./CommandPalette";
import styles from "./Layout.module.css";
import { Outline } from "./right-panel/Outline";
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

interface ParsedWikiLink {
  raw: string;
  target: string;
}

interface ParsedWikiLinkParts {
  alias: string | null;
  heading: string | null;
  target: string;
}

export interface RenameBacklinkRewriteResult {
  rewrittenDocuments: Document[];
  updatedDocuments: number;
  updatedLinks: number;
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

function parseWikiLinkParts(rawInner: string): ParsedWikiLinkParts | null {
  const trimmed = rawInner.trim();
  if (!trimmed) {
    return null;
  }

  const [targetWithHeadingRaw, aliasRaw] = trimmed.split("|", 2);
  const [targetRaw, headingRaw] = targetWithHeadingRaw.split("#", 2);
  const target = targetRaw.trim();
  if (!target) {
    return null;
  }

  const heading = headingRaw?.trim() || null;
  const alias = aliasRaw?.trim() || null;
  return { alias, heading, target };
}

function extractWikiLinks(markdown: string): ParsedWikiLink[] {
  const links: ParsedWikiLink[] = [];
  const pattern = /\[\[([^[\]]+)\]\]/g;
  let match: RegExpExecArray | null = pattern.exec(markdown);

  while (match) {
    const parsed = parseWikiLinkParts(match[1] ?? "");
    if (parsed) {
      const normalizedTarget = normalizeBacklinkTarget(parsed.target);
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

function targetAliases(
  document: Pick<Document, "path" | "title">,
): Set<string> {
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

function replacementTargetForRename(
  originalTarget: string,
  oldDocument: Pick<Document, "path" | "title">,
  nextPath: string,
): string {
  const normalizedOriginalTarget = normalizeBacklinkTarget(originalTarget);
  const normalizedOldPath = normalizeBacklinkTarget(oldDocument.path);
  const normalizedOldBaseName = normalizeBacklinkTarget(
    baseName(oldDocument.path),
  );
  const normalizedOldBaseNameWithoutExtension = normalizeBacklinkTarget(
    baseNameWithoutExtension(oldDocument.path),
  );
  const normalizedOldTitle = normalizeBacklinkTarget(oldDocument.title);

  if (normalizedOriginalTarget === normalizedOldPath) {
    return nextPath;
  }
  if (normalizedOriginalTarget === normalizedOldBaseName) {
    return baseName(nextPath);
  }
  if (
    normalizedOriginalTarget === normalizedOldBaseNameWithoutExtension ||
    normalizedOriginalTarget === normalizedOldTitle
  ) {
    return baseNameWithoutExtension(nextPath);
  }

  return nextPath;
}

export function rewriteWikiReferencesForRename(
  workspaceDocuments: readonly Document[],
  renamedDocument: Pick<Document, "id" | "path" | "title">,
  nextPath: string,
): RenameBacklinkRewriteResult {
  const trimmedNextPath = nextPath.trim();
  if (!trimmedNextPath) {
    return {
      rewrittenDocuments: [],
      updatedDocuments: 0,
      updatedLinks: 0,
    };
  }

  const oldAliases = targetAliases(renamedDocument);
  if (oldAliases.size === 0) {
    return {
      rewrittenDocuments: [],
      updatedDocuments: 0,
      updatedLinks: 0,
    };
  }

  const rewrittenDocuments: Document[] = [];
  let updatedLinks = 0;

  for (const document of workspaceDocuments) {
    if (
      document.id === renamedDocument.id ||
      typeof document.bodyMd !== "string"
    ) {
      continue;
    }

    let documentUpdatedLinks = 0;
    const rewrittenBody = document.bodyMd.replace(
      /\[\[([^[\]]+)\]\]/g,
      (rawMatch, rawInner: string) => {
        const parsed = parseWikiLinkParts(rawInner);
        if (!parsed) {
          return rawMatch;
        }

        const normalizedTarget = normalizeBacklinkTarget(parsed.target);
        if (!oldAliases.has(normalizedTarget)) {
          return rawMatch;
        }

        const replacementTarget = replacementTargetForRename(
          parsed.target,
          renamedDocument,
          trimmedNextPath,
        );
        let replacementInner = replacementTarget;
        if (parsed.heading) {
          replacementInner = `${replacementInner}#${parsed.heading}`;
        }
        if (parsed.alias) {
          replacementInner = `${replacementInner}|${parsed.alias}`;
        }
        documentUpdatedLinks += 1;
        return `[[${replacementInner}]]`;
      },
    );

    if (documentUpdatedLinks > 0) {
      updatedLinks += documentUpdatedLinks;
      rewrittenDocuments.push({
        ...document,
        bodyMd: rewrittenBody,
      });
    }
  }

  return {
    rewrittenDocuments,
    updatedDocuments: rewrittenDocuments.length,
    updatedLinks,
  };
}

export function formatRenameBacklinkToast(
  updatedLinks: number,
  updatedDocuments: number,
): string {
  return `Updated ${updatedLinks} links across ${updatedDocuments} documents.`;
}

export function buildIncomingBacklinks(
  documents: readonly Document[],
  activeDocumentId: string | null,
): IncomingBacklink[] {
  if (!activeDocumentId) {
    return [];
  }

  const activeDocument = documents.find(
    (document) => document.id === activeDocumentId,
  );
  if (!activeDocument) {
    return [];
  }
  const aliases = targetAliases(activeDocument);
  const backlinks: IncomingBacklink[] = [];

  for (const document of documents) {
    if (
      document.id === activeDocument.id ||
      typeof document.bodyMd !== "string"
    ) {
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
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [pendingRenameDocumentId, setPendingRenameDocumentId] = useState<
    string | null
  >(null);
  const [renameBacklinkToast, setRenameBacklinkToast] = useState<string | null>(
    null,
  );
  const [searchPanelOpen, setSearchPanelOpen] = useState(false);
  const [outlinePanelOpen, setOutlinePanelOpen] = useState(true);
  const [pendingDeleteDocument, setPendingDeleteDocument] =
    useState<Document | null>(null);
  const [outlineContainer, setOutlineContainer] = useState<HTMLElement | null>(
    null,
  );
  const editorAreaRef = useRef<HTMLElement | null>(null);

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
      setSearchPanelOpen(true);
    };

    if (typeof window === "undefined") {
      return undefined;
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [createUntitledDocument]);

  useEffect(() => {
    if (!renameBacklinkToast) {
      return;
    }

    const timeoutId = globalThis.setTimeout(() => {
      setRenameBacklinkToast(null);
    }, 3000);

    return () => {
      globalThis.clearTimeout(timeoutId);
    };
  }, [renameBacklinkToast]);

  useEffect(() => {
    if (!pendingDeleteDocument || typeof window === "undefined") {
      return;
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      setPendingDeleteDocument(null);
    };

    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [pendingDeleteDocument]);

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

    setRenameBacklinkToast(
      formatRenameBacklinkToast(updatedLinks, updatedDocuments),
    );
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
      <aside
        aria-label="Sidebar"
        className={styles.sidebar}
        data-testid="app-sidebar"
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
        {renameBacklinkToast ? (
          <p
            className={styles.renameBacklinkToast}
            data-testid="rename-backlink-toast"
            role="status"
          >
            {renameBacklinkToast}
          </p>
        ) : null}
        {searchPanelOpen ? (
          <SearchPanel
            onClose={() => setSearchPanelOpen(false)}
            onResultSelect={(result) =>
              handleSearchResultSelect(result.documentId)
            }
            results={searchPanelResults}
          />
        ) : (
          <section aria-label="Document tree section">
            <h2 className={styles.documentTreeHeading}>
              Documents
            </h2>
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
        className={styles.editorArea}
        data-testid="app-editor-area"
        ref={(node) => {
          editorAreaRef.current = node;
          setOutlineContainer(node);
        }}
      >
        <Outlet />
      </main>
      {outlinePanelOpen ? (
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
              onClick={() => setOutlinePanelOpen(false)}
              type="button"
            >
              Hide
            </button>
          </div>

          <Outline editorContainer={outlineContainer} />

          <section
            aria-label="Incoming backlinks"
            className={styles.backlinksSection}
            data-testid="backlinks-panel"
          >
            <h3 className={styles.backlinksTitle}>Backlinks</h3>
            {incomingBacklinks.length === 0 ? (
              <p className={styles.backlinksEmpty} data-testid="backlinks-empty">
                No incoming links to this document.
              </p>
            ) : (
              <ul
                aria-label="Incoming wiki links"
                className={styles.backlinksList}
                data-testid="backlinks-list"
              >
                {incomingBacklinks.map((backlink) => (
                  <li className={styles.backlinksItem} key={backlink.sourceDocumentId}>
                    <button
                      className={styles.backlinkButton}
                      data-testid={`backlink-item-${backlink.sourceDocumentId}`}
                      onClick={() =>
                        handleBacklinkSelect(backlink.sourceDocumentId)
                      }
                      type="button"
                    >
                      {backlink.sourceTitle}
                    </button>
                    <p
                      className={styles.backlinkSnippet}
                      data-testid={`backlink-snippet-${backlink.sourceDocumentId}`}
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
          className={styles.showOutlineButton}
          data-testid="outline-panel-toggle"
          onClick={() => setOutlinePanelOpen(true)}
          type="button"
        >
          Show Outline
        </button>
      )}
      {pendingDeleteDocument ? (
        <div
          className={styles.deleteOverlay}
          data-testid="delete-document-overlay"
        >
          <section
            aria-label="Delete document confirmation"
            aria-modal="true"
            className={styles.deleteDialog}
            data-testid="delete-document-dialog"
            role="alertdialog"
          >
            <h2 className={styles.deleteDialogTitle}>Delete document?</h2>
            <p className={styles.deleteDialogDescription}>
              Permanently delete <strong>{pendingDeleteDocument.path}</strong>?
              This cannot be undone.
            </p>
            <div className={styles.deleteDialogActions}>
              <button
                className={styles.secondaryButton}
                data-testid="delete-document-cancel"
                onClick={handleCancelDeleteDocument}
                type="button"
              >
                Cancel
              </button>
              <button
                className={styles.dangerButton}
                data-testid="delete-document-confirm"
                onClick={handleConfirmDeleteDocument}
                type="button"
              >
                Delete
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </div>
  );
}

export const AppLayout = Layout;
