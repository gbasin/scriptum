// @vitest-environment jsdom

import type { Document, Workspace } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useDocumentsStore } from "../store/documents";
import { useWorkspaceStore } from "../store/workspace";
import { WorkspaceRoute } from "./workspace";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeWorkspace(id: string, name: string): Workspace {
  return {
    id,
    slug: id,
    name,
    role: "owner",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    etag: `workspace-${id}-v1`,
  };
}

function makeDocument(
  id: string,
  workspaceId: string,
  title: string,
  updatedAt: string,
): Document {
  return {
    id,
    workspaceId,
    path: `${title.toLowerCase().replaceAll(" ", "-")}.md`,
    title,
    tags: [],
    headSeq: 1,
    etag: `${id}-v1`,
    archivedAt: null,
    deletedAt: null,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt,
  };
}

function renderRoute(path = "/workspace/ws-alpha") {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/workspace/:workspaceId" element={<WorkspaceRoute />} />
          <Route
            path="/workspace/:workspaceId/document/:documentId"
            element={<div data-testid="workspace-document-destination" />}
          />
          <Route
            path="/settings"
            element={<div data-testid="settings-page" />}
          />
        </Routes>
      </MemoryRouter>,
    );
  });

  return { container, root };
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  useWorkspaceStore.getState().reset();
  useDocumentsStore.getState().reset();
  useWorkspaceStore
    .getState()
    .setWorkspaces([makeWorkspace("ws-alpha", "Alpha Workspace")]);
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("WorkspaceRoute", () => {
  it("shows empty state and creates the first document", () => {
    const { container, root } = renderRoute();

    expect(
      container.querySelector('[data-testid="workspace-empty-state"]'),
    ).not.toBeNull();

    const createButton = container.querySelector(
      '[data-testid="workspace-create-first-document"]',
    ) as HTMLButtonElement | null;

    act(() => {
      createButton?.click();
    });

    const state = useDocumentsStore.getState();
    expect(state.documents).toHaveLength(1);
    expect(state.openDocumentIds).toHaveLength(1);
    expect(state.activeDocumentIdByWorkspace["ws-alpha"]).toBe(
      state.documents[0]?.id,
    );

    act(() => {
      root.unmount();
    });
  });

  it("renders document and recent-file sections for populated workspaces", () => {
    useDocumentsStore
      .getState()
      .setDocuments([
        makeDocument(
          "doc-new",
          "ws-alpha",
          "Newest",
          "2026-01-03T00:00:00.000Z",
        ),
        makeDocument(
          "doc-old",
          "ws-alpha",
          "Oldest",
          "2026-01-01T00:00:00.000Z",
        ),
      ]);

    const { container, root } = renderRoute();

    expect(
      container.querySelector('[data-testid="workspace-document-list"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="workspace-recent-files"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="workspace-settings-link"]'),
    ).not.toBeNull();

    const recentNewest = container.querySelector(
      '[data-testid="workspace-recent-doc-new"]',
    ) as HTMLButtonElement | null;
    expect(recentNewest?.textContent).toContain("Newest");

    act(() => {
      recentNewest?.click();
    });

    expect(useDocumentsStore.getState().openDocumentIds).toContain("doc-new");
    expect(useWorkspaceStore.getState().activeWorkspaceId).toBe("ws-alpha");

    act(() => {
      root.unmount();
    });
  });

  it("falls back to workspace id when metadata is missing", () => {
    const { container, root } = renderRoute("/workspace/missing-workspace");
    expect(container.textContent).toContain("Workspace: missing-workspace");

    act(() => {
      root.unmount();
    });
  });
});
