import type { Workspace } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import * as Y from "yjs";
import type { StateStorage } from "zustand/middleware";
import { bindWorkspaceStoreToYjs, createWorkspaceStore } from "./workspace";

const WORKSPACE_ALPHA: Workspace = {
  id: "ws-alpha",
  slug: "alpha",
  name: "Alpha",
  role: "owner",
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
  etag: "workspace-alpha-v1",
};

const WORKSPACE_BETA: Workspace = {
  id: "ws-beta",
  slug: "beta",
  name: "Beta",
  role: "editor",
  createdAt: "2026-01-02T00:00:00.000Z",
  updatedAt: "2026-01-02T00:00:00.000Z",
  etag: "workspace-beta-v1",
};

function createMemoryStorage(): StateStorage {
  const state = new Map<string, string>();

  return {
    getItem: (name) => state.get(name) ?? null,
    removeItem: (name) => {
      state.delete(name);
    },
    setItem: (name, value) => {
      state.set(name, value);
    },
  };
}

describe("workspace store", () => {
  it("tracks workspace list and active workspace with local actions", () => {
    const store = createWorkspaceStore();

    store.getState().setWorkspaces([WORKSPACE_ALPHA, WORKSPACE_BETA]);
    expect(
      store.getState().workspaces.map((workspace) => workspace.id),
    ).toEqual([WORKSPACE_ALPHA.id, WORKSPACE_BETA.id]);
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_ALPHA.id);
    expect(store.getState().activeWorkspace?.id).toBe(WORKSPACE_ALPHA.id);

    store.getState().setActiveWorkspaceId(WORKSPACE_BETA.id);
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_BETA.id);
    expect(store.getState().activeWorkspace?.id).toBe(WORKSPACE_BETA.id);

    store.getState().removeWorkspace(WORKSPACE_BETA.id);
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_ALPHA.id);
    expect(store.getState().activeWorkspace?.id).toBe(WORKSPACE_ALPHA.id);
  });

  it("reacts to Yjs updates", () => {
    const doc = new Y.Doc();
    const store = createWorkspaceStore();
    const stopBinding = bindWorkspaceStoreToYjs(doc, { store });
    const workspaces = doc.getArray<Workspace>("workspaces");
    const workspaceMeta = doc.getMap<unknown>("workspaceMeta");

    doc.transact(() => {
      workspaces.push([WORKSPACE_ALPHA, WORKSPACE_BETA]);
      workspaceMeta.set("activeWorkspaceId", WORKSPACE_BETA.id);
    });

    expect(
      store.getState().workspaces.map((workspace) => workspace.id),
    ).toEqual([WORKSPACE_ALPHA.id, WORKSPACE_BETA.id]);
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_BETA.id);

    doc.transact(() => {
      workspaces.delete(1, 1);
    });

    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_ALPHA.id);
    expect(store.getState().activeWorkspace?.id).toBe(WORKSPACE_ALPHA.id);

    stopBinding();
    doc.transact(() => {
      workspaces.delete(0, 1);
      workspaceMeta.set("activeWorkspaceId", null);
    });

    expect(
      store.getState().workspaces.map((workspace) => workspace.id),
    ).toEqual([WORKSPACE_ALPHA.id]);
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_ALPHA.id);
  });

  it("persists activeWorkspaceId across store instances", async () => {
    const persistStorage = createMemoryStorage();
    const persistKey = "workspace-persist-test";
    const source = createWorkspaceStore(
      { workspaces: [WORKSPACE_ALPHA, WORKSPACE_BETA] },
      { persistKey, persistStorage },
    );

    source.getState().setActiveWorkspaceId(WORKSPACE_BETA.id);

    const restored = createWorkspaceStore(
      { workspaces: [WORKSPACE_ALPHA, WORKSPACE_BETA] },
      { persistKey, persistStorage },
    ) as typeof source & {
      persist: { rehydrate: () => Promise<void> };
    };
    await restored.persist.rehydrate();

    expect(restored.getState().activeWorkspaceId).toBe(WORKSPACE_BETA.id);
    expect(restored.getState().activeWorkspace?.id).toBe(WORKSPACE_BETA.id);
  });

  it("updates existing workspaces via upsert without changing active workspace", () => {
    const store = createWorkspaceStore({
      workspaces: [WORKSPACE_ALPHA, WORKSPACE_BETA],
      activeWorkspaceId: WORKSPACE_ALPHA.id,
    });

    store.getState().upsertWorkspace({
      ...WORKSPACE_BETA,
      etag: "workspace-beta-v2",
      name: "Beta Renamed",
    });

    expect(
      store
        .getState()
        .workspaces.find((workspace) => workspace.id === WORKSPACE_BETA.id),
    ).toEqual({
      ...WORKSPACE_BETA,
      etag: "workspace-beta-v2",
      name: "Beta Renamed",
    });
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_ALPHA.id);
    expect(store.getState().activeWorkspace?.id).toBe(WORKSPACE_ALPHA.id);
  });

  it("falls back to the first workspace when active workspace id becomes invalid", () => {
    const store = createWorkspaceStore({
      workspaces: [WORKSPACE_ALPHA, WORKSPACE_BETA],
      activeWorkspaceId: WORKSPACE_BETA.id,
    });

    store.getState().setActiveWorkspaceId("missing-workspace");
    expect(store.getState().activeWorkspaceId).toBe(WORKSPACE_ALPHA.id);
    expect(store.getState().activeWorkspace?.id).toBe(WORKSPACE_ALPHA.id);
  });

  it("clears active workspace when the last workspace is removed", () => {
    const store = createWorkspaceStore({
      workspaces: [WORKSPACE_ALPHA],
      activeWorkspaceId: WORKSPACE_ALPHA.id,
    });

    store.getState().removeWorkspace(WORKSPACE_ALPHA.id);
    expect(store.getState().workspaces).toEqual([]);
    expect(store.getState().activeWorkspaceId).toBeNull();
    expect(store.getState().activeWorkspace).toBeNull();
  });
});
