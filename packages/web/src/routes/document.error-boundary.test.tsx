// @vitest-environment jsdom

import type { Document, Workspace } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDocumentsStore } from "../store/documents";
import { usePresenceStore } from "../store/presence";
import { useSyncStore } from "../store/sync";
import { useWorkspaceStore } from "../store/workspace";
import { DocumentRoute } from "./document";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

const listCommentsMock = vi.hoisted(() => vi.fn());
const getDocumentHistoryTimelineMock = vi.hoisted(() => vi.fn());
const getDocumentDiffTimelineMock = vi.hoisted(() => vi.fn());

vi.mock("../lib/api-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/api-client")>();
  return {
    ...actual,
    listComments: listCommentsMock,
    getDocumentHistoryTimeline: getDocumentHistoryTimelineMock,
    getDocumentDiffTimeline: getDocumentDiffTimelineMock,
  };
});

vi.mock("@scriptum/editor", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@scriptum/editor")>();
  return {
    ...actual,
    createCollaborationProvider: vi.fn(() => {
      throw new Error("editor-init-crash");
    }),
  };
});

function makeWorkspace(): Workspace {
  return {
    id: "ws-alpha",
    slug: "alpha",
    name: "Alpha Workspace",
    role: "owner",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    etag: "ws-alpha-v1",
  };
}

function makeDocument(): Document {
  return {
    id: "doc-auth",
    workspaceId: "ws-alpha",
    path: "docs/auth.md",
    title: "Auth",
    tags: [],
    headSeq: 0,
    etag: "doc-auth-v1",
    archivedAt: null,
    deletedAt: null,
    bodyMd: "# Auth\n\nDocument body",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

describe("DocumentRoute editor error boundary", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    useWorkspaceStore.getState().reset();
    useDocumentsStore.getState().reset();
    usePresenceStore.getState().reset();
    useSyncStore.getState().reset();

    const workspace = makeWorkspace();
    const document = makeDocument();
    useWorkspaceStore.getState().upsertWorkspace(workspace);
    useWorkspaceStore.getState().setActiveWorkspaceId(workspace.id);
    useDocumentsStore.getState().setDocuments([document]);
    useDocumentsStore.getState().openDocument(document.id);
    useDocumentsStore
      .getState()
      .setActiveDocumentForWorkspace(workspace.id, document.id);

    listCommentsMock.mockReset();
    getDocumentHistoryTimelineMock.mockReset();
    getDocumentDiffTimelineMock.mockReset();

    listCommentsMock.mockResolvedValue({
      items: [],
      nextCursor: null,
    });
    getDocumentHistoryTimelineMock.mockResolvedValue({
      events: [],
    });
    getDocumentDiffTimelineMock.mockResolvedValue({
      snapshots: [],
    });
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.restoreAllMocks();
  });

  it("keeps the document shell visible when editor initialization crashes", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter
          initialEntries={["/workspace/ws-alpha/document/doc-auth"]}
        >
          <Routes>
            <Route
              element={<DocumentRoute />}
              path="/workspace/:workspaceId/document/:documentId"
            />
          </Routes>
        </MemoryRouter>,
      );
    });

    await act(async () => {
      await Promise.resolve();
    });

    expect(
      container.querySelector('[data-testid="editor-error-boundary"]'),
    ).not.toBeNull();
    expect(container.textContent).toContain("Editor failed to load");
    expect(
      container.querySelector('[data-testid="presence-stack"]'),
    ).not.toBeNull();
    expect(consoleError).toHaveBeenCalled();

    act(() => {
      root.unmount();
    });
  });
});
