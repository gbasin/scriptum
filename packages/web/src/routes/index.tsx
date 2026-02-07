import type { Document, Workspace } from "@scriptum/shared";
import { useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../hooks/useAuth";
import { cn } from "../lib/cn";
import { useDocumentsStore } from "../store/documents";
import { useWorkspaceStore } from "../store/workspace";
import styles from "./index.module.css";

const MAX_RECENT_DOCUMENTS = 6;

interface RecentDocumentEntry {
  document: Document;
  workspaceName: string;
}

function parseTimestamp(iso: string): number {
  const timestamp = Date.parse(iso);
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

function makeWorkspace(
  existingCount: number,
  nowIso: string,
  token: string,
): Workspace {
  const workspaceId = `ws-${token}`;

  return {
    id: workspaceId,
    slug: workspaceId,
    name: `Workspace ${existingCount + 1}`,
    role: "owner",
    createdAt: nowIso,
    updatedAt: nowIso,
    etag: `workspace-${token}`,
  };
}

export function IndexRoute() {
  const navigate = useNavigate();
  const authLocation =
    typeof window === "undefined"
      ? {
          origin: "http://localhost",
          assign: (_url: string) => {},
        }
      : window.location;
  const { error, isAuthenticated, login, status, user } = useAuth({
    location: authLocation,
  });
  const workspaces = useWorkspaceStore((state) => state.workspaces);
  const activeWorkspaceId = useWorkspaceStore(
    (state) => state.activeWorkspaceId,
  );
  const setActiveWorkspaceId = useWorkspaceStore(
    (state) => state.setActiveWorkspaceId,
  );
  const upsertWorkspace = useWorkspaceStore((state) => state.upsertWorkspace);
  const documents = useDocumentsStore((state) => state.documents);
  const openDocument = useDocumentsStore((state) => state.openDocument);
  const setActiveDocumentForWorkspace = useDocumentsStore(
    (state) => state.setActiveDocumentForWorkspace,
  );

  const workspaceById = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.id, workspace])),
    [workspaces],
  );

  const recentDocuments = useMemo<RecentDocumentEntry[]>(() => {
    return documents
      .filter(
        (document) =>
          document.archivedAt === null && document.deletedAt === null,
      )
      .sort(
        (left, right) =>
          parseTimestamp(right.updatedAt) - parseTimestamp(left.updatedAt),
      )
      .slice(0, MAX_RECENT_DOCUMENTS)
      .map((document) => ({
        document,
        workspaceName:
          workspaceById.get(document.workspaceId)?.name ?? "Unknown workspace",
      }));
  }, [documents, workspaceById]);

  const openWorkspace = (workspaceId: string) => {
    setActiveWorkspaceId(workspaceId);
    navigate(`/workspace/${encodeURIComponent(workspaceId)}`);
  };

  const openRecentDocument = (document: Document) => {
    setActiveWorkspaceId(document.workspaceId);
    openDocument(document.id);
    setActiveDocumentForWorkspace(document.workspaceId, document.id);
    navigate(
      `/workspace/${encodeURIComponent(document.workspaceId)}/document/${encodeURIComponent(document.id)}`,
    );
  };

  const createWorkspace = () => {
    const token = Date.now().toString(36);
    const nowIso = new Date().toISOString();
    const workspace = makeWorkspace(workspaces.length, nowIso, token);

    upsertWorkspace(workspace);
    setActiveWorkspaceId(workspace.id);
    navigate(`/workspace/${encodeURIComponent(workspace.id)}`);
  };

  if (!isAuthenticated) {
    return (
      <main className={styles.page} data-testid="index-landing">
        <section className={styles.hero}>
          <p className={styles.eyebrow}>Local-first collaborative markdown</p>
          <h1 className={styles.title}>Scriptum</h1>
          <p className={styles.copy}>
            Collaborate in real time with humans and agents, then sync through
            git on your terms.
          </p>
          <button
            className={cn(styles.button, styles.primaryButton)}
            data-testid="index-login-button"
            onClick={() => {
              void login();
            }}
            type="button"
          >
            Sign in with GitHub
          </button>
          {error ? (
            <p className={styles.errorText} data-testid="index-login-error">
              {error}
            </p>
          ) : null}
          {status === "unknown" ? (
            <p className={styles.mutedText}>Checking your existing session…</p>
          ) : null}
        </section>
        <section className={styles.onboarding}>
          <h2 className={styles.sectionTitle}>What happens after sign in</h2>
          <ol className={styles.steps}>
            <li>Create or join a workspace.</li>
            <li>Edit markdown with CRDT-powered collaboration.</li>
            <li>Track agent contributions with first-class attribution.</li>
          </ol>
        </section>
      </main>
    );
  }

  return (
    <main className={styles.page} data-testid="index-authenticated">
      <header className={styles.header}>
        <div>
          <p className={styles.eyebrow}>Signed in as {user?.display_name}</p>
          <h1 className={styles.title}>Choose a workspace</h1>
        </div>
        <button
          className={cn(styles.button, styles.primaryButton)}
          data-testid="index-create-workspace-button"
          onClick={createWorkspace}
          type="button"
        >
          Create workspace
        </button>
      </header>

      <section className={styles.panel}>
        <h2 className={styles.sectionTitle}>Your workspaces</h2>
        {workspaces.length === 0 ? (
          <p className={styles.mutedText} data-testid="index-workspace-empty">
            No workspaces yet. Create one to get started.
          </p>
        ) : (
          <ul className={styles.list} data-testid="index-workspace-list">
            {workspaces.map((workspace) => (
              <li key={workspace.id}>
                <button
                  className={cn(styles.listButton, {
                    [styles.listButtonActive]:
                      workspace.id === activeWorkspaceId,
                  })}
                  data-testid={`index-workspace-item-${workspace.id}`}
                  onClick={() => openWorkspace(workspace.id)}
                  type="button"
                >
                  <span className={styles.listTitle}>{workspace.name}</span>
                  <span className={styles.listMeta}>
                    {workspace.role} · {workspace.slug}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className={styles.panel} data-testid="index-recent-documents">
        <h2 className={styles.sectionTitle}>Recent documents</h2>
        {recentDocuments.length === 0 ? (
          <p className={styles.mutedText}>No recent documents yet.</p>
        ) : (
          <ul className={styles.list}>
            {recentDocuments.map(({ document, workspaceName }) => (
              <li key={document.id}>
                <button
                  className={styles.listButton}
                  data-testid={`index-recent-document-${document.id}`}
                  onClick={() => openRecentDocument(document)}
                  type="button"
                >
                  <span className={styles.listTitle}>{document.title}</span>
                  <span className={styles.listMeta}>
                    {workspaceName} · {document.path}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}
