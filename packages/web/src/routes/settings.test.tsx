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
import { SettingsRoute } from "./settings";

const mockNavigate = vi.fn();

vi.mock("react-router-dom", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

vi.mock("../hooks/useAuth", () => ({
  useAuth: vi.fn(),
}));

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeWorkspace(): Workspace {
  return {
    createdAt: "2026-01-01T00:00:00.000Z",
    etag: "workspace-alpha-v1",
    id: "ws-alpha",
    name: "Alpha Workspace",
    role: "owner",
    slug: "alpha",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

function makeDocument(id: string, workspaceId: string): Document {
  return {
    archivedAt: null,
    createdAt: "2026-01-01T00:00:00.000Z",
    deletedAt: null,
    etag: `document-${id}-v1`,
    headSeq: 0,
    id,
    path: `docs/${id}.md`,
    tags: [],
    title: id,
    updatedAt: "2026-01-01T00:00:00.000Z",
    workspaceId,
  };
}

function authResult(overrides: Partial<UseAuthResult> = {}): UseAuthResult {
  return {
    status: "authenticated",
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: "Scriptum User",
    },
    accessToken: "access-token",
    error: null,
    isAuthenticated: true,
    login: vi.fn(async () => undefined),
    logout: vi.fn(async () => undefined),
    ...overrides,
  };
}

beforeEach(() => {
  useWorkspaceStore.getState().reset();
  useDocumentsStore.getState().reset();
  const workspace = makeWorkspace();
  useWorkspaceStore.getState().upsertWorkspace(workspace);
  useWorkspaceStore.getState().setActiveWorkspaceId(workspace.id);
  vi.mocked(useAuth).mockReturnValue(authResult());
  mockNavigate.mockReset();
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("SettingsRoute", () => {
  it("renders all settings tabs and switches panels", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter>
          <SettingsRoute />
        </MemoryRouter>,
      );
    });

    expect(
      container.querySelector('[data-testid="settings-tab-general"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="settings-tab-gitSync"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="settings-tab-agents"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="settings-tab-permissions"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="settings-tab-appearance"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="settings-form-general"]'),
    ).not.toBeNull();
    const generalTab = container.querySelector(
      '[data-testid="settings-tab-general"]',
    ) as HTMLButtonElement | null;
    const tabPanel = container.querySelector(
      '[data-testid="settings-tab-panel"]',
    ) as HTMLDivElement | null;
    expect(generalTab?.getAttribute("aria-controls")).toBe(
      "settings-panel-general",
    );
    expect(tabPanel?.getAttribute("id")).toBe("settings-panel-general");
    expect(tabPanel?.getAttribute("aria-labelledby")).toBe(
      "settings-tab-general",
    );

    const appearanceTab = container.querySelector(
      '[data-testid="settings-tab-appearance"]',
    ) as HTMLButtonElement | null;
    expect(appearanceTab?.getAttribute("aria-controls")).toBe(
      "settings-panel-appearance",
    );
    act(() => {
      appearanceTab?.click();
    });

    expect(
      container.querySelector('[data-testid="settings-form-general"]'),
    ).toBeNull();
    expect(
      container.querySelector('[data-testid="settings-form-appearance"]'),
    ).not.toBeNull();
    const updatedPanel = container.querySelector(
      '[data-testid="settings-tab-panel"]',
    ) as HTMLDivElement | null;
    expect(updatedPanel?.getAttribute("id")).toBe("settings-panel-appearance");
    expect(updatedPanel?.getAttribute("aria-labelledby")).toBe(
      "settings-tab-appearance",
    );

    act(() => {
      root.unmount();
    });
  });

  it("persists tab form changes to workspace config", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter>
          <SettingsRoute />
        </MemoryRouter>,
      );
    });

    const gitSyncTab = container.querySelector(
      '[data-testid="settings-tab-gitSync"]',
    ) as HTMLButtonElement | null;
    act(() => {
      gitSyncTab?.click();
    });

    const gitSyncEnabled = container.querySelector(
      '[data-testid="settings-git-sync-enabled"]',
    ) as HTMLInputElement | null;
    expect(gitSyncEnabled?.checked).toBe(true);
    act(() => {
      gitSyncEnabled?.click();
    });

    const appearanceTab = container.querySelector(
      '[data-testid="settings-tab-appearance"]',
    ) as HTMLButtonElement | null;
    act(() => {
      appearanceTab?.click();
    });

    const density = container.querySelector(
      '[data-testid="settings-appearance-density"]',
    ) as HTMLSelectElement | null;
    act(() => {
      if (!density) {
        return;
      }
      density.value = "spacious";
      density.dispatchEvent(new Event("change", { bubbles: true }));
    });

    const fontFamily = container.querySelector(
      '[data-testid="settings-editor-font-family"]',
    ) as HTMLSelectElement | null;
    act(() => {
      if (!fontFamily) {
        return;
      }
      fontFamily.value = "sans";
      fontFamily.dispatchEvent(new Event("change", { bubbles: true }));
    });

    const lineNumbers = container.querySelector(
      '[data-testid="settings-editor-line-numbers"]',
    ) as HTMLInputElement | null;
    expect(lineNumbers?.checked).toBe(true);
    act(() => {
      lineNumbers?.click();
    });

    const persistedWorkspace = useWorkspaceStore.getState().activeWorkspace;
    expect(persistedWorkspace?.config?.gitSync.enabled).toBe(false);
    expect(persistedWorkspace?.config?.gitSync.autoCommitIntervalSeconds).toBe(
      30,
    );
    expect(persistedWorkspace?.config?.appearance.density).toBe("spacious");
    expect(persistedWorkspace?.config?.appearance.fontSize).toBe(15);
    expect(persistedWorkspace?.config?.editor.fontFamily).toBe("sans");
    expect(persistedWorkspace?.config?.editor.tabSize).toBe(2);
    expect(persistedWorkspace?.config?.editor.lineNumbers).toBe(false);

    act(() => {
      root.unmount();
    });
  });

  it("logs out and navigates back to index", async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const logout = vi.fn(async () => undefined);
    vi.mocked(useAuth).mockReturnValue(authResult({ logout }));
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter>
          <SettingsRoute />
        </MemoryRouter>,
      );
    });

    const logoutButton = container.querySelector(
      '[data-testid="settings-logout"]',
    ) as HTMLButtonElement | null;
    expect(logoutButton).not.toBeNull();

    await act(async () => {
      logoutButton?.click();
    });

    expect(logout).toHaveBeenCalledTimes(1);
    expect(mockNavigate).toHaveBeenCalledWith("/", { replace: true });

    act(() => {
      root.unmount();
    });
  });

  it("prevents deleting the last workspace", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter>
          <SettingsRoute />
        </MemoryRouter>,
      );
    });

    const deleteButton = container.querySelector(
      '[data-testid="settings-delete-workspace"]',
    ) as HTMLButtonElement | null;
    expect(deleteButton?.disabled).toBe(true);
    expect(
      container.querySelector(
        '[data-testid="settings-delete-workspace-last-warning"]',
      ),
    ).not.toBeNull();

    act(() => {
      root.unmount();
    });
  });

  it("deletes the active workspace and removes its documents", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const betaWorkspace: Workspace = {
      ...makeWorkspace(),
      etag: "workspace-beta-v1",
      id: "ws-beta",
      name: "Beta Workspace",
      slug: "beta",
      updatedAt: "2026-01-02T00:00:00.000Z",
    };
    useWorkspaceStore.getState().upsertWorkspace(betaWorkspace);
    useDocumentsStore
      .getState()
      .setDocuments([
        makeDocument("doc-alpha", "ws-alpha"),
        makeDocument("doc-beta", "ws-beta"),
      ]);

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter>
          <SettingsRoute />
        </MemoryRouter>,
      );
    });

    const deleteButton = container.querySelector(
      '[data-testid="settings-delete-workspace"]',
    ) as HTMLButtonElement | null;
    expect(deleteButton?.disabled).toBe(false);
    act(() => {
      deleteButton?.click();
    });

    expect(
      container.querySelector(
        '[data-testid="settings-delete-workspace-confirm"]',
      ),
    ).not.toBeNull();

    const confirmDeleteButton = container.querySelector(
      '[data-testid="settings-delete-workspace-confirm-action"]',
    ) as HTMLButtonElement | null;
    act(() => {
      confirmDeleteButton?.click();
    });

    expect(
      useWorkspaceStore.getState().workspaces.map((workspace) => workspace.id),
    ).toEqual(["ws-beta"]);
    expect(useWorkspaceStore.getState().activeWorkspaceId).toBe("ws-beta");
    expect(
      useDocumentsStore
        .getState()
        .documents.map((document) => document.workspaceId),
    ).toEqual(["ws-beta"]);
    expect(mockNavigate).toHaveBeenCalledWith("/workspace/ws-beta", {
      replace: true,
    });

    act(() => {
      root.unmount();
    });
  });
});
