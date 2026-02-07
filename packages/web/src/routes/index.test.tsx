// @vitest-environment jsdom

import type { Document, Workspace } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UseAuthResult } from "../hooks/useAuth";
import { useAuth } from "../hooks/useAuth";
import { useDocumentsStore } from "../store/documents";
import { useWorkspaceStore } from "../store/workspace";
import { IndexRoute } from "./index";

vi.mock("../hooks/useAuth", () => ({
  useAuth: vi.fn(),
}));

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeWorkspace(id: string, name: string, role = "owner"): Workspace {
  return {
    id,
    slug: id,
    name,
    role,
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

function authResult(overrides: Partial<UseAuthResult> = {}): UseAuthResult {
  return {
    status: "unauthenticated",
    user: null,
    accessToken: null,
    error: null,
    isAuthenticated: false,
    login: vi.fn(async () => undefined),
    logout: vi.fn(async () => undefined),
    ...overrides,
  };
}

function renderRoute() {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(
      <MemoryRouter>
        <IndexRoute />
      </MemoryRouter>,
    );
  });

  return { container, root };
}

beforeEach(() => {
  useWorkspaceStore.getState().reset();
  useDocumentsStore.getState().reset();
  vi.mocked(useAuth).mockReturnValue(authResult());
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("IndexRoute", () => {
  it("renders unauthenticated landing and starts OAuth on click", () => {
    const login = vi.fn(async () => undefined);
    vi.mocked(useAuth).mockReturnValue(authResult({ login }));
    const { container, root } = renderRoute();

    expect(
      container.querySelector('[data-testid="index-landing"]'),
    ).not.toBeNull();
    const button = container.querySelector(
      '[data-testid="index-login-button"]',
    ) as HTMLButtonElement | null;
    expect(button?.textContent).toContain("Sign in with GitHub");

    act(() => {
      button?.click();
    });
    expect(login).toHaveBeenCalledTimes(1);

    act(() => {
      root.unmount();
    });
  });

  it("renders workspace list and creates a new workspace for authenticated users", () => {
    vi.mocked(useAuth).mockReturnValue(
      authResult({
        status: "authenticated",
        isAuthenticated: true,
        user: {
          id: "user-1",
          email: "user@example.com",
          display_name: "Scriptum User",
        },
      }),
    );

    useWorkspaceStore
      .getState()
      .setWorkspaces([
        makeWorkspace("ws-alpha", "Alpha Workspace"),
        makeWorkspace("ws-beta", "Beta Workspace", "editor"),
      ]);
    useWorkspaceStore.getState().setActiveWorkspaceId("ws-alpha");
    useDocumentsStore
      .getState()
      .setDocuments([
        makeDocument(
          "doc-newest",
          "ws-beta",
          "Newest Note",
          "2026-01-03T00:00:00.000Z",
        ),
        makeDocument(
          "doc-oldest",
          "ws-alpha",
          "Oldest Note",
          "2026-01-01T00:00:00.000Z",
        ),
      ]);

    const { container, root } = renderRoute();

    expect(
      container.querySelector('[data-testid="index-workspace-list"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="index-workspace-item-ws-alpha"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="index-workspace-item-ws-beta"]'),
    ).not.toBeNull();

    const createButton = container.querySelector(
      '[data-testid="index-create-workspace-button"]',
    ) as HTMLButtonElement | null;
    act(() => {
      createButton?.click();
    });

    const workspaces = useWorkspaceStore.getState().workspaces;
    expect(workspaces).toHaveLength(3);
    expect(workspaces[2]?.name).toBe("Workspace 3");
    expect(useWorkspaceStore.getState().activeWorkspaceId).toBe(
      workspaces[2]?.id ?? null,
    );

    act(() => {
      root.unmount();
    });
  });

  it("opens recent documents and syncs active workspace/document state", () => {
    vi.mocked(useAuth).mockReturnValue(
      authResult({
        status: "authenticated",
        isAuthenticated: true,
        user: {
          id: "user-1",
          email: "user@example.com",
          display_name: "Scriptum User",
        },
      }),
    );

    useWorkspaceStore
      .getState()
      .setWorkspaces([
        makeWorkspace("ws-alpha", "Alpha Workspace"),
        makeWorkspace("ws-beta", "Beta Workspace", "editor"),
      ]);
    useWorkspaceStore.getState().setActiveWorkspaceId("ws-alpha");
    useDocumentsStore
      .getState()
      .setDocuments([
        makeDocument(
          "doc-newest",
          "ws-beta",
          "Newest Note",
          "2026-01-03T00:00:00.000Z",
        ),
        makeDocument(
          "doc-mid",
          "ws-alpha",
          "Mid Note",
          "2026-01-02T00:00:00.000Z",
        ),
      ]);

    const { container, root } = renderRoute();
    const newestButton = container.querySelector(
      '[data-testid="index-recent-document-doc-newest"]',
    ) as HTMLButtonElement | null;
    const midButton = container.querySelector(
      '[data-testid="index-recent-document-doc-mid"]',
    ) as HTMLButtonElement | null;

    expect(newestButton?.textContent).toContain("Newest Note");
    expect(midButton?.textContent).toContain("Mid Note");

    act(() => {
      newestButton?.click();
    });

    expect(useWorkspaceStore.getState().activeWorkspaceId).toBe("ws-beta");
    expect(
      useDocumentsStore.getState().activeDocumentIdByWorkspace["ws-beta"],
    ).toBe("doc-newest");
    expect(useDocumentsStore.getState().openDocumentIds).toContain(
      "doc-newest",
    );

    act(() => {
      root.unmount();
    });
  });
});
