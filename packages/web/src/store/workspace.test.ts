import type { Workspace } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import * as Y from "yjs";
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
});
