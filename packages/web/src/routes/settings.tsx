import type { WorkspaceConfig } from "@scriptum/shared";
import { useMemo, useState } from "react";
import { useWorkspaceStore } from "../store/workspace";

type SettingsTab =
  | "general"
  | "gitSync"
  | "agents"
  | "permissions"
  | "appearance";

interface SettingsTabDefinition {
  id: SettingsTab;
  label: string;
}

const SETTINGS_TABS: SettingsTabDefinition[] = [
  { id: "general", label: "General" },
  { id: "gitSync", label: "Git Sync" },
  { id: "agents", label: "Agents" },
  { id: "permissions", label: "Permissions" },
  { id: "appearance", label: "Appearance" },
];

export function defaultWorkspaceConfig(workspaceName: string): WorkspaceConfig {
  return {
    general: {
      workspaceName,
      defaultNewDocumentFolder: "notes",
      openLastDocumentOnLaunch: true,
    },
    gitSync: {
      enabled: true,
      autoCommitIntervalSeconds: 30,
      commitMessageTemplate: "docs: sync workspace edits",
    },
    agents: {
      allowAgentEdits: true,
      requireSectionLease: true,
      defaultAgentName: "mcp-agent",
    },
    permissions: {
      defaultRole: "editor",
      allowExternalInvites: false,
      allowShareLinks: true,
    },
    appearance: {
      theme: "system",
      density: "comfortable",
      editorFontSizePx: 15,
    },
  };
}

function asPositiveInt(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

export function SettingsRoute() {
  const activeWorkspaceId = useWorkspaceStore(
    (state) => state.activeWorkspaceId,
  );
  const workspaces = useWorkspaceStore((state) => state.workspaces);
  const upsertWorkspace = useWorkspaceStore((state) => state.upsertWorkspace);
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");

  const activeWorkspace = useMemo(
    () =>
      activeWorkspaceId
        ? (workspaces.find((workspace) => workspace.id === activeWorkspaceId) ??
          null)
        : null,
    [activeWorkspaceId, workspaces],
  );

  if (!activeWorkspace) {
    return (
      <section aria-label="Settings" data-testid="settings-page">
        <h1>Settings</h1>
        <p data-testid="settings-empty">No active workspace selected.</p>
      </section>
    );
  }

  const config =
    activeWorkspace.config ?? defaultWorkspaceConfig(activeWorkspace.name);

  const persist = (
    nextConfig: WorkspaceConfig,
    options: { workspaceName?: string } = {},
  ) => {
    const nextWorkspaceName =
      options.workspaceName && options.workspaceName.trim().length > 0
        ? options.workspaceName.trim()
        : activeWorkspace.name;
    const now = new Date().toISOString();
    upsertWorkspace({
      ...activeWorkspace,
      config: nextConfig,
      etag: `${activeWorkspace.etag}:settings:${Date.now().toString(36)}`,
      name: nextWorkspaceName,
      updatedAt: now,
    });
  };

  const updateConfig = (
    mutate: (current: WorkspaceConfig) => WorkspaceConfig,
  ) => {
    const nextConfig = mutate(config);
    persist(nextConfig);
  };

  return (
    <section aria-label="Settings" data-testid="settings-page">
      <h1>Settings</h1>
      <p data-testid="settings-workspace-name">
        Workspace: <strong>{activeWorkspace.name}</strong>
      </p>

      <div
        aria-label="Settings tabs"
        data-testid="settings-tabs"
        role="tablist"
        style={{
          display: "flex",
          gap: "0.5rem",
          marginBottom: "1rem",
          marginTop: "1rem",
        }}
      >
        {SETTINGS_TABS.map((tab) => (
          <button
            aria-selected={activeTab === tab.id}
            data-testid={`settings-tab-${tab.id}`}
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            role="tab"
            type="button"
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div
        aria-label="Settings tab panel"
        data-testid="settings-tab-panel"
        role="tabpanel"
      >
        {activeTab === "general" ? (
          <fieldset data-testid="settings-form-general">
            <legend>General</legend>
            <label>
              Workspace name
              <input
                data-testid="settings-general-workspace-name"
                onChange={(event) => {
                  const workspaceName = event.target.value;
                  const nextConfig: WorkspaceConfig = {
                    ...config,
                    general: { ...config.general, workspaceName },
                  };
                  persist(nextConfig, { workspaceName });
                }}
                type="text"
                value={config.general.workspaceName}
              />
            </label>
            <label>
              Default new document folder
              <input
                data-testid="settings-general-default-folder"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    general: {
                      ...current.general,
                      defaultNewDocumentFolder: event.target.value,
                    },
                  }))
                }
                type="text"
                value={config.general.defaultNewDocumentFolder}
              />
            </label>
            <label>
              <input
                checked={config.general.openLastDocumentOnLaunch}
                data-testid="settings-general-open-last-document"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    general: {
                      ...current.general,
                      openLastDocumentOnLaunch: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Open last document on launch
            </label>
          </fieldset>
        ) : null}

        {activeTab === "gitSync" ? (
          <fieldset data-testid="settings-form-git-sync">
            <legend>Git Sync</legend>
            <label>
              <input
                checked={config.gitSync.enabled}
                data-testid="settings-git-sync-enabled"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    gitSync: {
                      ...current.gitSync,
                      enabled: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Enable git sync
            </label>
            <label>
              Auto commit interval (seconds)
              <input
                data-testid="settings-git-sync-interval"
                min={5}
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    gitSync: {
                      ...current.gitSync,
                      autoCommitIntervalSeconds: asPositiveInt(
                        event.target.value,
                        current.gitSync.autoCommitIntervalSeconds,
                      ),
                    },
                  }))
                }
                type="number"
                value={config.gitSync.autoCommitIntervalSeconds}
              />
            </label>
            <label>
              Commit message template
              <input
                data-testid="settings-git-sync-message-template"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    gitSync: {
                      ...current.gitSync,
                      commitMessageTemplate: event.target.value,
                    },
                  }))
                }
                type="text"
                value={config.gitSync.commitMessageTemplate}
              />
            </label>
          </fieldset>
        ) : null}

        {activeTab === "agents" ? (
          <fieldset data-testid="settings-form-agents">
            <legend>Agents</legend>
            <label>
              <input
                checked={config.agents.allowAgentEdits}
                data-testid="settings-agents-allow-edits"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    agents: {
                      ...current.agents,
                      allowAgentEdits: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Allow agent edits
            </label>
            <label>
              <input
                checked={config.agents.requireSectionLease}
                data-testid="settings-agents-require-lease"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    agents: {
                      ...current.agents,
                      requireSectionLease: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Require section lease
            </label>
            <label>
              Default agent name
              <input
                data-testid="settings-agents-default-name"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    agents: {
                      ...current.agents,
                      defaultAgentName: event.target.value,
                    },
                  }))
                }
                type="text"
                value={config.agents.defaultAgentName}
              />
            </label>
          </fieldset>
        ) : null}

        {activeTab === "permissions" ? (
          <fieldset data-testid="settings-form-permissions">
            <legend>Permissions</legend>
            <label>
              Default role
              <select
                data-testid="settings-permissions-default-role"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    permissions: {
                      ...current.permissions,
                      defaultRole:
                        event.target.value === "viewer" ? "viewer" : "editor",
                    },
                  }))
                }
                value={config.permissions.defaultRole}
              >
                <option value="viewer">Viewer</option>
                <option value="editor">Editor</option>
              </select>
            </label>
            <label>
              <input
                checked={config.permissions.allowExternalInvites}
                data-testid="settings-permissions-allow-invites"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    permissions: {
                      ...current.permissions,
                      allowExternalInvites: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Allow external invites
            </label>
            <label>
              <input
                checked={config.permissions.allowShareLinks}
                data-testid="settings-permissions-allow-share-links"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    permissions: {
                      ...current.permissions,
                      allowShareLinks: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Allow share links
            </label>
          </fieldset>
        ) : null}

        {activeTab === "appearance" ? (
          <fieldset data-testid="settings-form-appearance">
            <legend>Appearance</legend>
            <label>
              Theme
              <select
                data-testid="settings-appearance-theme"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    appearance: {
                      ...current.appearance,
                      theme:
                        event.target.value === "light"
                          ? "light"
                          : event.target.value === "dark"
                            ? "dark"
                            : "system",
                    },
                  }))
                }
                value={config.appearance.theme}
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
            <label>
              Density
              <select
                data-testid="settings-appearance-density"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    appearance: {
                      ...current.appearance,
                      density:
                        event.target.value === "compact"
                          ? "compact"
                          : "comfortable",
                    },
                  }))
                }
                value={config.appearance.density}
              >
                <option value="comfortable">Comfortable</option>
                <option value="compact">Compact</option>
              </select>
            </label>
            <label>
              Editor font size (px)
              <input
                data-testid="settings-appearance-font-size"
                min={10}
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    appearance: {
                      ...current.appearance,
                      editorFontSizePx: asPositiveInt(
                        event.target.value,
                        current.appearance.editorFontSizePx,
                      ),
                    },
                  }))
                }
                type="number"
                value={config.appearance.editorFontSizePx}
              />
            </label>
          </fieldset>
        ) : null}
      </div>
    </section>
  );
}
