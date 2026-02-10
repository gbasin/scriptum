import type { Document, Workspace } from "@scriptum/shared";
import clsx from "clsx";
import { useMemo, useRef } from "react";
import { useNavigate } from "react-router-dom";
import {
  type OnboardingResult,
  type OnboardingTemplate,
  OnboardingWizard,
} from "../components/onboarding/OnboardingWizard";
import { useAuth } from "../hooks/useAuth";
import { titleFromPath } from "../lib/document-utils";
import { isTauri } from "../lib/tauri-auth";
import { useDocumentsStore } from "../store/documents";
import { useRuntimeStore } from "../store/runtime";
import { useUiStore } from "../store/ui";
import { useWorkspaceStore } from "../store/workspace";
import styles from "./index.module.css";

const MAX_RECENT_DOCUMENTS = 6;
const ONBOARDING_FIRST_VISIT_STORAGE_KEY = "scriptum:onboarding:first-visit";

interface RecentDocumentEntry {
  document: Document;
  workspaceName: string;
}

interface TemplateBlueprint {
  bodyMd: string;
  path: string;
}

const TEMPLATE_BLUEPRINTS: Record<OnboardingTemplate, TemplateBlueprint> = {
  blank: {
    path: "untitled-1.md",
    bodyMd: "",
  },
  "meeting-notes": {
    path: "meeting-notes.md",
    bodyMd: [
      "# Meeting notes",
      "",
      "## Agenda",
      "-",
      "",
      "## Notes",
      "-",
      "",
      "## Action items",
      "- [ ] Owner — task",
    ].join("\n"),
  },
  "spec-template": {
    path: "spec-template.md",
    bodyMd: [
      "# Specification",
      "",
      "## Problem",
      "",
      "## Goals",
      "",
      "## Non-goals",
      "",
      "## Design",
      "",
      "## Rollout plan",
    ].join("\n"),
  },
};

function parseTimestamp(iso: string): number {
  const timestamp = Date.parse(iso);
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

function consumeFirstVisitFlag(): boolean {
  try {
    if (typeof globalThis.localStorage === "undefined") {
      return false;
    }

    const candidate = globalThis.localStorage as Partial<Storage>;
    if (
      typeof candidate.getItem !== "function" ||
      typeof candidate.setItem !== "function"
    ) {
      return false;
    }

    const hasVisited = candidate.getItem(ONBOARDING_FIRST_VISIT_STORAGE_KEY);
    if (hasVisited === "1") {
      return false;
    }

    candidate.setItem(ONBOARDING_FIRST_VISIT_STORAGE_KEY, "1");
    return true;
  } catch {
    return false;
  }
}

function buildUniquePath(basePath: string, existingPaths: ReadonlySet<string>) {
  if (!existingPaths.has(basePath)) {
    return basePath;
  }

  const extensionIndex = basePath.lastIndexOf(".");
  const hasExtension = extensionIndex > 0;
  const prefix = hasExtension ? basePath.slice(0, extensionIndex) : basePath;
  const extension = hasExtension ? basePath.slice(extensionIndex) : "";
  let suffix = 2;
  let candidate = `${prefix}-${suffix}${extension}`;

  while (existingPaths.has(candidate)) {
    suffix += 1;
    candidate = `${prefix}-${suffix}${extension}`;
  }

  return candidate;
}

function makeWorkspace(
  existingCount: number,
  nowIso: string,
  token: string,
  workspaceName?: string,
): Workspace {
  const workspaceId = `ws-${token}`;
  const trimmedName = workspaceName?.trim();

  return {
    id: workspaceId,
    slug: workspaceId,
    name:
      trimmedName && trimmedName.length > 0
        ? trimmedName
        : `Workspace ${existingCount + 1}`,
    role: "owner",
    createdAt: nowIso,
    updatedAt: nowIso,
    etag: `workspace-${token}`,
  };
}

function makeDocumentFromTemplate(
  workspaceId: string,
  nowIso: string,
  token: string,
  template: OnboardingTemplate,
  existingPaths: ReadonlySet<string>,
): Document {
  const blueprint = TEMPLATE_BLUEPRINTS[template];
  const path = buildUniquePath(blueprint.path, existingPaths);

  return {
    id: `doc-${token}`,
    workspaceId,
    path,
    title: titleFromPath(path),
    tags: [],
    headSeq: 0,
    etag: `document-${token}`,
    archivedAt: null,
    deletedAt: null,
    createdAt: nowIso,
    updatedAt: nowIso,
    bodyMd: blueprint.bodyMd,
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
  const runtimeMode = useRuntimeStore((state) => state.mode);
  const onboardingCompleted = useUiStore((state) => state.onboardingCompleted);
  const completeOnboarding = useUiStore((state) => state.completeOnboarding);
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
  const upsertDocument = useDocumentsStore((state) => state.upsertDocument);
  const setActiveDocumentForWorkspace = useDocumentsStore(
    (state) => state.setActiveDocumentForWorkspace,
  );
  const firstVisitRef = useRef<boolean | null>(null);

  if (isAuthenticated && firstVisitRef.current === null) {
    firstVisitRef.current = consumeFirstVisitFlag();
  }

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

  const skipOnboarding = () => {
    completeOnboarding();
  };

  const completeOnboardingFlow = (result: OnboardingResult) => {
    const token = `${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6)
      .toString(36)
      .padStart(4, "0")}`;
    const nowIso = new Date().toISOString();
    const workspace = makeWorkspace(
      workspaces.length,
      nowIso,
      token,
      result.workspaceName,
    );

    upsertWorkspace(workspace);
    setActiveWorkspaceId(workspace.id);

    const existingPaths = new Set(
      documents
        .filter((document) => document.workspaceId === workspace.id)
        .map((document) => document.path),
    );
    const document = makeDocumentFromTemplate(
      workspace.id,
      nowIso,
      token,
      result.template,
      existingPaths,
    );
    upsertDocument(document);
    openDocument(document.id);
    setActiveDocumentForWorkspace(workspace.id, document.id);

    completeOnboarding();
    navigate(
      `/workspace/${encodeURIComponent(workspace.id)}/document/${encodeURIComponent(document.id)}`,
    );
  };

  const shouldShowOnboarding =
    isAuthenticated &&
    !onboardingCompleted &&
    ((firstVisitRef.current ?? false) || workspaces.length === 0);

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
            className={clsx(styles.button, styles.primaryButton)}
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

  if (shouldShowOnboarding) {
    return (
      <main className={styles.page} data-testid="index-onboarding">
        <OnboardingWizard
          defaultWorkspaceName={`Workspace ${workspaces.length + 1}`}
          desktopRuntime={isTauri()}
          onComplete={completeOnboardingFlow}
          onSkip={skipOnboarding}
          runtimeMode={runtimeMode}
        />
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
          className={clsx(styles.button, styles.primaryButton)}
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
                  className={clsx(styles.listButton, {
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
