import type { Workspace } from "@scriptum/shared";
import { create, type StoreApi, type UseBoundStore } from "zustand";
import * as Y from "yjs";

const DEFAULT_WORKSPACES_ARRAY_NAME = "workspaces";
const DEFAULT_WORKSPACE_META_MAP_NAME = "workspaceMeta";
const DEFAULT_ACTIVE_WORKSPACE_ID_KEY = "activeWorkspaceId";

interface WorkspaceSnapshot {
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
}

interface ResolvedWorkspaceSnapshot extends WorkspaceSnapshot {
  activeWorkspace: Workspace | null;
}

export interface WorkspaceStoreState extends ResolvedWorkspaceSnapshot {
  setWorkspaces: (workspaces: Workspace[]) => void;
  upsertWorkspace: (workspace: Workspace) => void;
  removeWorkspace: (workspaceId: string) => void;
  setActiveWorkspaceId: (workspaceId: string | null) => void;
  reset: () => void;
}

export type WorkspaceStore = UseBoundStore<StoreApi<WorkspaceStoreState>>;

export interface WorkspaceYjsBindingOptions {
  activeWorkspaceIdKey?: string;
  store?: WorkspaceStore;
  workspaceMetaMapName?: string;
  workspacesArrayName?: string;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function normalizeWorkspace(value: unknown): Workspace | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const workspace = value as Record<string, unknown>;
  const id = asString(workspace.id);
  const slug = asString(workspace.slug);
  const name = asString(workspace.name);
  const role = asString(workspace.role);
  const createdAt = asString(workspace.createdAt);
  const updatedAt = asString(workspace.updatedAt);
  const etag = asString(workspace.etag);
  if (!id || !slug || !name || !role || !createdAt || !updatedAt || !etag) {
    return null;
  }

  return {
    id,
    slug,
    name,
    role,
    createdAt,
    updatedAt,
    etag,
  };
}

function normalizeWorkspaces(values: readonly unknown[]): Workspace[] {
  const workspaces: Workspace[] = [];
  const seenWorkspaceIds = new Set<string>();

  for (const value of values) {
    const workspace = normalizeWorkspace(value);
    if (!workspace || seenWorkspaceIds.has(workspace.id)) {
      continue;
    }

    seenWorkspaceIds.add(workspace.id);
    workspaces.push(workspace);
  }

  return workspaces;
}

function resolveWorkspaceSnapshot(
  snapshot: WorkspaceSnapshot
): ResolvedWorkspaceSnapshot {
  const workspaces = snapshot.workspaces.slice();
  const workspaceById = new Map(
    workspaces.map((workspace) => [workspace.id, workspace])
  );
  const activeWorkspaceId =
    snapshot.activeWorkspaceId && workspaceById.has(snapshot.activeWorkspaceId)
      ? snapshot.activeWorkspaceId
      : workspaces[0]?.id ?? null;

  return {
    workspaces,
    activeWorkspaceId,
    activeWorkspace: activeWorkspaceId
      ? workspaceById.get(activeWorkspaceId) ?? null
      : null,
  };
}

export function createWorkspaceStore(
  initial: Partial<WorkspaceSnapshot> = {}
): WorkspaceStore {
  return create<WorkspaceStoreState>()((set, get) => ({
    ...resolveWorkspaceSnapshot({
      workspaces: initial.workspaces ?? [],
      activeWorkspaceId: initial.activeWorkspaceId ?? null,
    }),
    setWorkspaces: (workspaces) => {
      const previous = get();
      set(
        resolveWorkspaceSnapshot({
          workspaces,
          activeWorkspaceId: previous.activeWorkspaceId,
        })
      );
    },
    upsertWorkspace: (workspace) => {
      const previous = get();
      const index = previous.workspaces.findIndex(
        (candidate) => candidate.id === workspace.id
      );
      const workspaces =
        index >= 0
          ? previous.workspaces.map((candidate) =>
              candidate.id === workspace.id ? workspace : candidate
            )
          : [...previous.workspaces, workspace];

      set(
        resolveWorkspaceSnapshot({
          workspaces,
          activeWorkspaceId: previous.activeWorkspaceId ?? workspace.id,
        })
      );
    },
    removeWorkspace: (workspaceId) => {
      const previous = get();
      const workspaces = previous.workspaces.filter(
        (workspace) => workspace.id !== workspaceId
      );
      set(
        resolveWorkspaceSnapshot({
          workspaces,
          activeWorkspaceId: previous.activeWorkspaceId,
        })
      );
    },
    setActiveWorkspaceId: (workspaceId) => {
      const previous = get();
      set(
        resolveWorkspaceSnapshot({
          workspaces: previous.workspaces,
          activeWorkspaceId: workspaceId,
        })
      );
    },
    reset: () =>
      set(
        resolveWorkspaceSnapshot({
          workspaces: [],
          activeWorkspaceId: null,
        })
      ),
  }));
}

export const useWorkspaceStore = createWorkspaceStore();

export function bindWorkspaceStoreToYjs(
  doc: Y.Doc,
  options: WorkspaceYjsBindingOptions = {}
): () => void {
  const store = options.store ?? useWorkspaceStore;
  const workspacesArray = doc.getArray<unknown>(
    options.workspacesArrayName ?? DEFAULT_WORKSPACES_ARRAY_NAME
  );
  const workspaceMeta = doc.getMap<unknown>(
    options.workspaceMetaMapName ?? DEFAULT_WORKSPACE_META_MAP_NAME
  );
  const activeWorkspaceIdKey =
    options.activeWorkspaceIdKey ?? DEFAULT_ACTIVE_WORKSPACE_ID_KEY;

  const syncFromYjs = () => {
    const workspaces = normalizeWorkspaces(workspacesArray.toArray());
    const activeWorkspaceIdValue = workspaceMeta.get(activeWorkspaceIdKey);
    const activeWorkspaceId =
      typeof activeWorkspaceIdValue === "string" ? activeWorkspaceIdValue : null;

    store.setState(
      resolveWorkspaceSnapshot({
        workspaces,
        activeWorkspaceId,
      })
    );
  };

  const handleWorkspacesChange = () => syncFromYjs();
  const handleWorkspaceMetaChange = () => syncFromYjs();

  workspacesArray.observe(handleWorkspacesChange);
  workspaceMeta.observe(handleWorkspaceMetaChange);
  syncFromYjs();

  return () => {
    workspacesArray.unobserve(handleWorkspacesChange);
    workspaceMeta.unobserve(handleWorkspaceMetaChange);
  };
}

