import type { Document } from "@scriptum/shared";
import { useMemo } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { cn } from "../lib/cn";
import { useDocumentsStore } from "../store/documents";
import { useWorkspaceStore } from "../store/workspace";
import styles from "./workspace.module.css";

const MAX_RECENT_FILES = 5;

function parseTimestamp(iso: string): number {
  const timestamp = Date.parse(iso);
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

function titleFromPath(path: string): string {
  const segments = path.split("/");
  const lastSegment = segments[segments.length - 1] ?? "";
  const nameWithoutExtension = lastSegment.replace(/\.md$/i, "");
  return nameWithoutExtension.length > 0 ? nameWithoutExtension : path;
}

function buildUntitledPath(existingPaths: Set<string>): string {
  let suffix = 1;
  let candidatePath = "untitled-1.md";

  while (existingPaths.has(candidatePath)) {
    suffix += 1;
    candidatePath = `untitled-${suffix}.md`;
  }

  return candidatePath;
}

export function WorkspaceRoute() {
  const { workspaceId } = useParams();
  const navigate = useNavigate();
  const workspaces = useWorkspaceStore((state) => state.workspaces);
  const setActiveWorkspaceId = useWorkspaceStore(
    (state) => state.setActiveWorkspaceId,
  );
  const documents = useDocumentsStore((state) => state.documents);
  const openDocument = useDocumentsStore((state) => state.openDocument);
  const setActiveDocumentForWorkspace = useDocumentsStore(
    (state) => state.setActiveDocumentForWorkspace,
  );
  const upsertDocument = useDocumentsStore((state) => state.upsertDocument);

  const workspace = useMemo(
    () =>
      workspaces.find(
        (candidate) =>
          candidate.id === workspaceId || candidate.slug === workspaceId,
      ) ?? null,
    [workspaceId, workspaces],
  );

  const resolvedWorkspaceId = workspace?.id ?? (workspaceId ?? "unknown");
  const workspaceLabel = workspace?.name ?? (workspaceId ?? "unknown");

  const workspaceDocuments = useMemo(() => {
    return documents
      .filter(
        (document) =>
          document.workspaceId === resolvedWorkspaceId &&
          document.deletedAt === null &&
          document.archivedAt === null,
      )
      .sort(
        (left, right) =>
          parseTimestamp(right.updatedAt) - parseTimestamp(left.updatedAt),
      );
  }, [documents, resolvedWorkspaceId]);

  const recentFiles = workspaceDocuments.slice(0, MAX_RECENT_FILES);

  const openWorkspaceDocument = (document: Document) => {
    setActiveWorkspaceId(resolvedWorkspaceId);
    openDocument(document.id);
    setActiveDocumentForWorkspace(resolvedWorkspaceId, document.id);
    navigate(
      `/workspace/${encodeURIComponent(resolvedWorkspaceId)}/document/${encodeURIComponent(document.id)}`,
    );
  };

  const createFirstDocument = () => {
    const existingPaths = new Set(
      workspaceDocuments.map((document) => document.path),
    );
    const path = buildUntitledPath(existingPaths);
    const nowIso = new Date().toISOString();
    const token = `${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6)
      .toString(36)
      .padStart(4, "0")}`;
    const documentId = `doc-${token}`;

    const document: Document = {
      id: documentId,
      workspaceId: resolvedWorkspaceId,
      path,
      title: titleFromPath(path),
      tags: [],
      headSeq: 0,
      etag: `document-${token}`,
      archivedAt: null,
      deletedAt: null,
      createdAt: nowIso,
      updatedAt: nowIso,
      bodyMd: "",
    };

    upsertDocument(document);
    openWorkspaceDocument(document);
  };

  return (
    <section className={styles.route} data-testid="workspace-route">
      <header className={styles.header}>
        <div>
          <p className={styles.eyebrow}>Workspace overview</p>
          <h1 className={styles.title}>Workspace: {workspaceLabel}</h1>
        </div>
        <Link
          className={cn(styles.linkButton, styles.settingsButton)}
          data-testid="workspace-settings-link"
          to="/settings"
        >
          Workspace settings
        </Link>
      </header>

      {workspaceDocuments.length === 0 ? (
        <div className={styles.emptyState} data-testid="workspace-empty-state">
          <h2 className={styles.sectionTitle}>No documents yet</h2>
          <p className={styles.mutedText}>
            Create your first document to start writing in this workspace.
          </p>
          <button
            className={cn(styles.linkButton, styles.primaryButton)}
            data-testid="workspace-create-first-document"
            onClick={createFirstDocument}
            type="button"
          >
            Create your first document
          </button>
        </div>
      ) : (
        <div className={styles.content}>
          <section className={styles.panel}>
            <h2 className={styles.sectionTitle}>Documents</h2>
            <ul className={styles.list} data-testid="workspace-document-list">
              {workspaceDocuments.map((document) => (
                <li key={document.id}>
                  <button
                    className={styles.listButton}
                    data-testid={`workspace-document-${document.id}`}
                    onClick={() => openWorkspaceDocument(document)}
                    type="button"
                  >
                    <span className={styles.listTitle}>{document.title}</span>
                    <span className={styles.listMeta}>{document.path}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>

          <section className={styles.panel} data-testid="workspace-recent-files">
            <h2 className={styles.sectionTitle}>Recent files</h2>
            <ul className={styles.list}>
              {recentFiles.map((document) => (
                <li key={document.id}>
                  <button
                    className={styles.listButton}
                    data-testid={`workspace-recent-${document.id}`}
                    onClick={() => openWorkspaceDocument(document)}
                    type="button"
                  >
                    <span className={styles.listTitle}>{document.title}</span>
                    <span className={styles.listMeta}>
                      Updated {new Date(document.updatedAt).toLocaleString()}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        </div>
      )}
    </section>
  );
}
