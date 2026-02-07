// @vitest-environment jsdom

import type { Workspace } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useWorkspaceStore } from "../store/workspace";
import { SettingsRoute } from "./settings";

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

beforeEach(() => {
  useWorkspaceStore.getState().reset();
  const workspace = makeWorkspace();
  useWorkspaceStore.getState().upsertWorkspace(workspace);
  useWorkspaceStore.getState().setActiveWorkspaceId(workspace.id);
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

    const appearanceTab = container.querySelector(
      '[data-testid="settings-tab-appearance"]',
    ) as HTMLButtonElement | null;
    act(() => {
      appearanceTab?.click();
    });

    expect(
      container.querySelector('[data-testid="settings-form-general"]'),
    ).toBeNull();
    expect(
      container.querySelector('[data-testid="settings-form-appearance"]'),
    ).not.toBeNull();

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
});
