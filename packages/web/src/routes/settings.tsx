import type {
  GitSyncPolicy,
  WorkspaceConfig,
  WorkspaceMember,
} from "@scriptum/shared";
import clsx from "clsx";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useAuth } from "../hooks/useAuth";
import {
  configureGitSyncSettings,
  getGitSyncSettings,
  inviteToWorkspace,
  listMembers,
  removeMember,
  updateMember,
} from "../lib/api-client";
import { useDocumentsStore } from "../store/documents";
import { defaultWorkspaceConfig, useWorkspaceStore } from "../store/workspace";
import controls from "../styles/Controls.module.css";
import styles from "./settings.module.css";

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

function asPositiveInt(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

interface GitSyncFormState {
  remoteUrl: string;
  branch: string;
  pushPolicy: GitSyncPolicy;
  aiCommitEnabled: boolean;
  commitIntervalSeconds: string;
}

interface GitSyncStatusState {
  dirty: boolean;
  ahead: number;
  behind: number;
  lastSyncAt: string | null;
}

function isGitSyncPolicy(value: string): value is GitSyncPolicy {
  return value === "disabled" || value === "manual" || value === "auto_rebase";
}

function gitSyncFormFromWorkspaceConfig(
  config: WorkspaceConfig,
): GitSyncFormState {
  return {
    remoteUrl: "origin",
    branch: "main",
    pushPolicy: config.gitSync.enabled ? "manual" : "disabled",
    aiCommitEnabled: true,
    commitIntervalSeconds: String(
      asPositiveInt(String(config.gitSync.autoCommitIntervalSeconds), 30),
    ),
  };
}

function formatLastSyncAt(lastSyncAt: string | null): string {
  if (!lastSyncAt) {
    return "Never";
  }
  const timestamp = Date.parse(lastSyncAt);
  if (Number.isNaN(timestamp)) {
    return "Unknown";
  }
  return new Date(timestamp).toLocaleString();
}

function requestErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }
  return "Request failed.";
}

function memberStatus(member: WorkspaceMember): string {
  return member.status?.trim().toLowerCase() || "active";
}

export function SettingsRoute() {
  const navigate = useNavigate();
  const { logout } = useAuth();
  const activeWorkspaceId = useWorkspaceStore(
    (state) => state.activeWorkspaceId,
  );
  const workspaces = useWorkspaceStore((state) => state.workspaces);
  const setActiveWorkspaceId = useWorkspaceStore(
    (state) => state.setActiveWorkspaceId,
  );
  const removeWorkspace = useWorkspaceStore((state) => state.removeWorkspace);
  const upsertWorkspace = useWorkspaceStore((state) => state.upsertWorkspace);
  const documents = useDocumentsStore((state) => state.documents);
  const setDocuments = useDocumentsStore((state) => state.setDocuments);
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [isDeleteConfirmOpen, setIsDeleteConfirmOpen] = useState(false);
  const [gitSyncForm, setGitSyncForm] = useState<GitSyncFormState>({
    remoteUrl: "origin",
    branch: "main",
    pushPolicy: "manual",
    aiCommitEnabled: true,
    commitIntervalSeconds: "30",
  });
  const [gitSyncStatus, setGitSyncStatus] = useState<GitSyncStatusState>({
    dirty: false,
    ahead: 0,
    behind: 0,
    lastSyncAt: null,
  });
  const [gitSyncLoading, setGitSyncLoading] = useState(false);
  const [gitSyncSaving, setGitSyncSaving] = useState(false);
  const [gitSyncError, setGitSyncError] = useState<string | null>(null);
  const [gitSyncNotice, setGitSyncNotice] = useState<string | null>(null);
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviteRole, setInviteRole] = useState<"editor" | "viewer">("editor");
  const [members, setMembers] = useState<WorkspaceMember[]>([]);
  const [membersLoading, setMembersLoading] = useState(false);
  const [membersError, setMembersError] = useState<string | null>(null);
  const [inviteBusy, setInviteBusy] = useState(false);
  const [inviteError, setInviteError] = useState<string | null>(null);
  const [inviteNotice, setInviteNotice] = useState<string | null>(null);
  const [updatingMemberId, setUpdatingMemberId] = useState<string | null>(null);
  const [removingMemberId, setRemovingMemberId] = useState<string | null>(null);
  const [pendingRemoveMemberId, setPendingRemoveMemberId] = useState<
    string | null
  >(null);
  const lastGitSyncWorkspaceIdRef = useRef<string | null>(null);

  const activeWorkspace = useMemo(
    () =>
      activeWorkspaceId
        ? (workspaces.find((workspace) => workspace.id === activeWorkspaceId) ??
          null)
        : null,
    [activeWorkspaceId, workspaces],
  );

  useEffect(() => {
    setIsDeleteConfirmOpen(false);
  }, [activeWorkspaceId]);

  useEffect(() => {
    if (!activeWorkspace) {
      lastGitSyncWorkspaceIdRef.current = null;
      return;
    }
    if (lastGitSyncWorkspaceIdRef.current === activeWorkspace.id) {
      return;
    }

    lastGitSyncWorkspaceIdRef.current = activeWorkspace.id;
    const workspaceConfig =
      activeWorkspace.config ?? defaultWorkspaceConfig(activeWorkspace.name);
    setGitSyncForm(gitSyncFormFromWorkspaceConfig(workspaceConfig));
    setGitSyncStatus({
      dirty: false,
      ahead: 0,
      behind: 0,
      lastSyncAt: null,
    });
    setGitSyncError(null);
    setGitSyncNotice(null);
    setInviteEmail("");
    setInviteRole("editor");
    setMembers([]);
    setMembersError(null);
    setInviteError(null);
    setInviteNotice(null);
    setUpdatingMemberId(null);
    setRemovingMemberId(null);
    setPendingRemoveMemberId(null);
  }, [activeWorkspace]);

  useEffect(() => {
    if (!activeWorkspace || activeTab !== "gitSync") {
      return;
    }

    let cancelled = false;
    setGitSyncLoading(true);
    setGitSyncError(null);
    void getGitSyncSettings(activeWorkspace.id)
      .then((snapshot) => {
        if (cancelled) {
          return;
        }
        setGitSyncForm((current) => ({
          ...current,
          remoteUrl: snapshot.remoteUrl,
          branch: snapshot.branch,
          pushPolicy: snapshot.pushPolicy,
          aiCommitEnabled: snapshot.aiCommitEnabled,
          commitIntervalSeconds: String(snapshot.commitIntervalSeconds),
        }));
        setGitSyncStatus({
          dirty: snapshot.dirty,
          ahead: snapshot.ahead,
          behind: snapshot.behind,
          lastSyncAt: snapshot.lastSyncAt,
        });
      })
      .catch((error: unknown) => {
        if (cancelled) {
          return;
        }
        setGitSyncError(
          error instanceof Error
            ? error.message
            : "Failed to load Git Sync settings from daemon.",
        );
      })
      .finally(() => {
        if (!cancelled) {
          setGitSyncLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [activeTab, activeWorkspace]);

  const loadMembersForActiveWorkspace = useCallback(async () => {
    if (!activeWorkspace) {
      setMembers([]);
      return;
    }

    setMembersLoading(true);
    setMembersError(null);
    try {
      const response = await listMembers(activeWorkspace.id);
      setMembers(response.items);
    } catch (error: unknown) {
      setMembersError(requestErrorMessage(error));
    } finally {
      setMembersLoading(false);
    }
  }, [activeWorkspace]);

  useEffect(() => {
    if (!activeWorkspace || activeTab !== "permissions") {
      return;
    }
    void loadMembersForActiveWorkspace();
  }, [activeTab, activeWorkspace, loadMembersForActiveWorkspace]);

  if (!activeWorkspace) {
    return (
      <section
        aria-label="Settings"
        className={styles.page}
        data-testid="settings-page"
      >
        <h1 className={styles.heading}>Settings</h1>
        <p className={styles.emptyMessage} data-testid="settings-empty">
          No active workspace selected.
        </p>
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

  const handleLogout = async () => {
    try {
      await logout();
    } finally {
      navigate("/", { replace: true });
    }
  };

  const otherWorkspaces = workspaces.filter(
    (workspace) => workspace.id !== activeWorkspace.id,
  );
  const canDeleteWorkspace = otherWorkspaces.length > 0;
  const fallbackWorkspace = otherWorkspaces[0] ?? null;

  const handleDeleteWorkspace = () => {
    if (!fallbackWorkspace) {
      return;
    }

    removeWorkspace(activeWorkspace.id);
    setDocuments(
      documents.filter(
        (document) => document.workspaceId !== activeWorkspace.id,
      ),
    );
    setActiveWorkspaceId(fallbackWorkspace.id);
    setIsDeleteConfirmOpen(false);
    navigate(`/workspace/${encodeURIComponent(fallbackWorkspace.id)}`, {
      replace: true,
    });
  };

  const handleSaveGitSync = async () => {
    const commitIntervalSeconds = asPositiveInt(
      gitSyncForm.commitIntervalSeconds,
      config.gitSync.autoCommitIntervalSeconds,
    );
    setGitSyncSaving(true);
    setGitSyncError(null);
    setGitSyncNotice(null);

    try {
      const saved = await configureGitSyncSettings(activeWorkspace.id, {
        remoteUrl: gitSyncForm.remoteUrl,
        branch: gitSyncForm.branch,
        pushPolicy: gitSyncForm.pushPolicy,
        aiCommitEnabled: gitSyncForm.aiCommitEnabled,
        commitIntervalSeconds,
      });
      setGitSyncForm({
        remoteUrl: saved.remoteUrl,
        branch: saved.branch,
        pushPolicy: saved.pushPolicy,
        aiCommitEnabled: saved.aiCommitEnabled,
        commitIntervalSeconds: String(saved.commitIntervalSeconds),
      });
      updateConfig((current) => ({
        ...current,
        gitSync: {
          ...current.gitSync,
          enabled: saved.pushPolicy !== "disabled",
          autoCommitIntervalSeconds: saved.commitIntervalSeconds,
        },
      }));
      setGitSyncNotice("Git sync settings saved.");
    } catch (error: unknown) {
      setGitSyncError(
        error instanceof Error
          ? error.message
          : "Failed to save Git Sync settings to daemon.",
      );
    } finally {
      setGitSyncSaving(false);
    }
  };

  const handleInviteMember = async () => {
    const email = inviteEmail.trim();
    if (!email) {
      setInviteError("Email is required.");
      return;
    }

    setInviteBusy(true);
    setInviteError(null);
    setInviteNotice(null);
    try {
      await inviteToWorkspace(activeWorkspace.id, {
        email,
        role: inviteRole,
      });
      setInviteEmail("");
      setInviteNotice("Invitation sent.");
      await loadMembersForActiveWorkspace();
    } catch (error: unknown) {
      setInviteError(requestErrorMessage(error));
    } finally {
      setInviteBusy(false);
    }
  };

  const handleUpdateMemberRole = async (
    member: WorkspaceMember,
    nextRole: "editor" | "viewer",
  ) => {
    setUpdatingMemberId(member.user_id);
    setInviteError(null);
    setInviteNotice(null);
    try {
      await updateMember(activeWorkspace.id, member.user_id, {
        role: nextRole,
      });
      setInviteNotice("Member role updated.");
      await loadMembersForActiveWorkspace();
    } catch (error: unknown) {
      setInviteError(requestErrorMessage(error));
    } finally {
      setUpdatingMemberId(null);
    }
  };

  const handleRemoveMember = async (member: WorkspaceMember) => {
    setRemovingMemberId(member.user_id);
    setInviteError(null);
    setInviteNotice(null);
    try {
      await removeMember(activeWorkspace.id, member.user_id);
      setPendingRemoveMemberId(null);
      setInviteNotice(
        memberStatus(member) === "invited"
          ? "Pending invite revoked."
          : "Member removed.",
      );
      await loadMembersForActiveWorkspace();
    } catch (error: unknown) {
      setInviteError(requestErrorMessage(error));
    } finally {
      setRemovingMemberId(null);
    }
  };

  return (
    <section
      aria-label="Settings"
      className={styles.page}
      data-testid="settings-page"
    >
      <h1 className={styles.heading}>Settings</h1>
      <p className={styles.workspaceName} data-testid="settings-workspace-name">
        Workspace: <strong>{activeWorkspace.name}</strong>
      </p>
      <div className={styles.accountActions}>
        <button
          className={clsx(controls.buttonBase, controls.buttonDanger)}
          data-testid="settings-logout"
          onClick={() => {
            void handleLogout();
          }}
          type="button"
        >
          Log out
        </button>
      </div>
      <section className={styles.dangerZone} data-testid="settings-danger-zone">
        <h2 className={styles.legend}>Danger zone</h2>
        <p
          className={styles.dangerMessage}
          data-testid="settings-delete-workspace-message"
        >
          Delete this workspace and all of its local documents.
        </p>
        <button
          className={clsx(controls.buttonBase, controls.buttonDanger)}
          data-testid="settings-delete-workspace"
          disabled={!canDeleteWorkspace}
          onClick={() => {
            if (!canDeleteWorkspace) {
              return;
            }
            setIsDeleteConfirmOpen((current) => !current);
          }}
          type="button"
        >
          Delete workspace
        </button>
        {!canDeleteWorkspace ? (
          <p
            className={styles.warningText}
            data-testid="settings-delete-workspace-last-warning"
          >
            Create another workspace before deleting this one.
          </p>
        ) : null}
        {isDeleteConfirmOpen ? (
          <div
            className={styles.confirmActions}
            data-testid="settings-delete-workspace-confirm"
          >
            <p className={styles.warningText}>This action cannot be undone.</p>
            <div className={styles.accountActions}>
              <button
                className={clsx(controls.buttonBase, controls.buttonSecondary)}
                data-testid="settings-delete-workspace-cancel"
                onClick={() => setIsDeleteConfirmOpen(false)}
                type="button"
              >
                Cancel
              </button>
              <button
                className={clsx(controls.buttonBase, controls.buttonDanger)}
                data-testid="settings-delete-workspace-confirm-action"
                onClick={handleDeleteWorkspace}
                type="button"
              >
                Confirm delete
              </button>
            </div>
          </div>
        ) : null}
      </section>

      <div
        aria-label="Settings tabs"
        className={styles.tabs}
        data-testid="settings-tabs"
        role="tablist"
      >
        {SETTINGS_TABS.map((tab) => (
          <button
            aria-controls={`settings-panel-${tab.id}`}
            aria-selected={activeTab === tab.id}
            className={clsx(
              controls.buttonBase,
              styles.tabButton,
              activeTab === tab.id
                ? styles.tabButtonActive
                : controls.buttonSecondary,
            )}
            data-testid={`settings-tab-${tab.id}`}
            id={`settings-tab-${tab.id}`}
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
        aria-labelledby={`settings-tab-${activeTab}`}
        className={styles.tabPanel}
        data-testid="settings-tab-panel"
        id={`settings-panel-${activeTab}`}
        role="tabpanel"
      >
        {activeTab === "general" ? (
          <fieldset
            className={styles.formSection}
            data-testid="settings-form-general"
          >
            <legend className={styles.legend}>General</legend>
            <label className={controls.field}>
              Workspace name
              <input
                className={controls.textInput}
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
            <label className={controls.field}>
              Default new document folder
              <input
                className={controls.textInput}
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
            <label className={controls.checkboxRow}>
              <input
                className={controls.checkbox}
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
          <fieldset
            className={styles.formSection}
            data-testid="settings-form-git-sync"
          >
            <legend className={styles.legend}>Git Sync</legend>
            <div
              className={styles.statusGrid}
              data-testid="settings-git-sync-state"
            >
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>Remote</span>
                <span data-testid="settings-git-sync-state-remote">
                  {gitSyncForm.remoteUrl}
                </span>
              </p>
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>Branch</span>
                <span data-testid="settings-git-sync-state-branch">
                  {gitSyncForm.branch}
                </span>
              </p>
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>Push policy</span>
                <span data-testid="settings-git-sync-state-policy">
                  {gitSyncForm.pushPolicy}
                </span>
              </p>
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>AI commits</span>
                <span data-testid="settings-git-sync-state-ai">
                  {gitSyncForm.aiCommitEnabled ? "Enabled" : "Disabled"}
                </span>
              </p>
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>Dirty</span>
                <span data-testid="settings-git-sync-state-dirty">
                  {gitSyncStatus.dirty ? "Yes" : "No"}
                </span>
              </p>
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>Ahead/behind</span>
                <span data-testid="settings-git-sync-state-ahead-behind">
                  {gitSyncStatus.ahead}/{gitSyncStatus.behind}
                </span>
              </p>
              <p className={styles.statusRow}>
                <span className={styles.statusLabel}>Last sync</span>
                <span data-testid="settings-git-sync-state-last-sync">
                  {formatLastSyncAt(gitSyncStatus.lastSyncAt)}
                </span>
              </p>
            </div>
            {gitSyncLoading ? (
              <p
                className={styles.helperText}
                data-testid="settings-git-sync-loading"
              >
                Loading Git Sync settings from daemon…
              </p>
            ) : null}
            {gitSyncError ? (
              <p
                className={styles.errorText}
                data-testid="settings-git-sync-error"
              >
                {gitSyncError}
              </p>
            ) : null}
            {gitSyncNotice ? (
              <p
                className={styles.successText}
                data-testid="settings-git-sync-notice"
              >
                {gitSyncNotice}
              </p>
            ) : null}
            <label className={controls.field}>
              Remote URL
              <input
                className={controls.textInput}
                data-testid="settings-git-sync-remote-url"
                onChange={(event) =>
                  setGitSyncForm((current) => ({
                    ...current,
                    remoteUrl: event.target.value,
                  }))
                }
                type="text"
                value={gitSyncForm.remoteUrl}
              />
            </label>
            <label className={controls.field}>
              Branch name
              <input
                className={controls.textInput}
                data-testid="settings-git-sync-branch"
                onChange={(event) =>
                  setGitSyncForm((current) => ({
                    ...current,
                    branch: event.target.value,
                  }))
                }
                type="text"
                value={gitSyncForm.branch}
              />
            </label>
            <label className={controls.field}>
              Push policy
              <select
                className={controls.selectInput}
                data-testid="settings-git-sync-push-policy"
                onChange={(event) =>
                  setGitSyncForm((current) => ({
                    ...current,
                    pushPolicy: isGitSyncPolicy(event.target.value)
                      ? event.target.value
                      : "manual",
                  }))
                }
                value={gitSyncForm.pushPolicy}
              >
                <option value="disabled">disabled</option>
                <option value="manual">manual</option>
                <option value="auto_rebase">auto_rebase</option>
              </select>
            </label>
            <label className={controls.checkboxRow}>
              <input
                checked={gitSyncForm.aiCommitEnabled}
                className={controls.checkbox}
                data-testid="settings-git-sync-ai-commit"
                onChange={(event) =>
                  setGitSyncForm((current) => ({
                    ...current,
                    aiCommitEnabled: event.target.checked,
                  }))
                }
                type="checkbox"
              />
              Enable AI commit messages
            </label>
            <label className={controls.field}>
              Commit interval (seconds)
              <input
                className={controls.textInput}
                data-testid="settings-git-sync-interval"
                min={5}
                onChange={(event) =>
                  setGitSyncForm((current) => ({
                    ...current,
                    commitIntervalSeconds: event.target.value,
                  }))
                }
                type="number"
                value={gitSyncForm.commitIntervalSeconds}
              />
            </label>
            <div className={styles.formActions}>
              <button
                className={clsx(controls.buttonBase, controls.buttonPrimary)}
                data-testid="settings-git-sync-save"
                disabled={gitSyncSaving}
                onClick={() => {
                  void handleSaveGitSync();
                }}
                type="button"
              >
                {gitSyncSaving ? "Saving…" : "Save Git Sync Settings"}
              </button>
            </div>
          </fieldset>
        ) : null}

        {activeTab === "agents" ? (
          <fieldset
            className={styles.formSection}
            data-testid="settings-form-agents"
          >
            <legend className={styles.legend}>Agents</legend>
            <label className={controls.checkboxRow}>
              <input
                className={controls.checkbox}
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
            <label className={controls.checkboxRow}>
              <input
                className={controls.checkbox}
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
            <label className={controls.field}>
              Default agent name
              <input
                className={controls.textInput}
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
          <fieldset
            className={styles.formSection}
            data-testid="settings-form-permissions"
          >
            <legend className={styles.legend}>Permissions</legend>
            <div className={styles.permissionsSection}>
              <label className={controls.field}>
                Default role
                <select
                  className={controls.selectInput}
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
              <label className={controls.checkboxRow}>
                <input
                  className={controls.checkbox}
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
              <label className={controls.checkboxRow}>
                <input
                  className={controls.checkbox}
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
            </div>

            <div className={styles.permissionsSection}>
              <h3 className={styles.subheading}>Invite Member</h3>
              <label className={controls.field}>
                Email
                <input
                  className={controls.textInput}
                  data-testid="settings-permissions-invite-email"
                  onChange={(event) => setInviteEmail(event.target.value)}
                  type="email"
                  value={inviteEmail}
                />
              </label>
              <label className={controls.field}>
                Role
                <select
                  className={controls.selectInput}
                  data-testid="settings-permissions-invite-role"
                  onChange={(event) =>
                    setInviteRole(
                      event.target.value === "viewer" ? "viewer" : "editor",
                    )
                  }
                  value={inviteRole}
                >
                  <option value="editor">editor</option>
                  <option value="viewer">viewer</option>
                </select>
              </label>
              <div className={styles.formActions}>
                <button
                  className={clsx(controls.buttonBase, controls.buttonPrimary)}
                  data-testid="settings-permissions-invite-submit"
                  disabled={inviteBusy}
                  onClick={() => {
                    void handleInviteMember();
                  }}
                  type="button"
                >
                  {inviteBusy ? "Sending…" : "Send Invite"}
                </button>
              </div>
              {inviteError ? (
                <p
                  className={styles.errorText}
                  data-testid="settings-permissions-invite-error"
                >
                  {inviteError}
                </p>
              ) : null}
              {inviteNotice ? (
                <p
                  className={styles.successText}
                  data-testid="settings-permissions-invite-notice"
                >
                  {inviteNotice}
                </p>
              ) : null}
            </div>

            <div className={styles.permissionsSection}>
              <h3 className={styles.subheading}>Members</h3>
              {membersLoading ? (
                <p
                  className={styles.helperText}
                  data-testid="settings-permissions-members-loading"
                >
                  Loading members…
                </p>
              ) : null}
              {membersError ? (
                <p
                  className={styles.errorText}
                  data-testid="settings-permissions-members-error"
                >
                  {membersError}
                </p>
              ) : null}
              {members.length === 0 && !membersLoading ? (
                <p
                  className={styles.helperText}
                  data-testid="settings-permissions-members-empty"
                >
                  No members found.
                </p>
              ) : null}
              {members.length > 0 ? (
                <ul
                  className={styles.memberList}
                  data-testid="settings-permissions-members"
                >
                  {members.map((member) => {
                    const status = memberStatus(member);
                    const memberId = member.user_id;
                    const pendingRemoval = pendingRemoveMemberId === memberId;
                    return (
                      <li
                        className={styles.memberItem}
                        data-testid={`settings-permissions-member-${memberId}`}
                        key={memberId}
                      >
                        <div className={styles.memberIdentity}>
                          <strong>{member.email}</strong>
                          <span className={styles.memberMeta}>{status}</span>
                        </div>
                        <div className={styles.memberControls}>
                          <label className={controls.field}>
                            Role
                            <select
                              className={controls.selectInput}
                              data-testid={`settings-permissions-member-role-${memberId}`}
                              disabled={updatingMemberId === memberId}
                              onChange={(event) => {
                                const nextRole =
                                  event.target.value === "viewer"
                                    ? "viewer"
                                    : "editor";
                                void handleUpdateMemberRole(member, nextRole);
                              }}
                              value={
                                member.role === "viewer" ? "viewer" : "editor"
                              }
                            >
                              <option value="editor">editor</option>
                              <option value="viewer">viewer</option>
                            </select>
                          </label>
                          {pendingRemoval ? (
                            <div
                              className={styles.confirmActions}
                              data-testid={`settings-permissions-member-remove-confirm-${memberId}`}
                            >
                              <p className={styles.warningText}>
                                {status === "invited"
                                  ? "Revoke this pending invite?"
                                  : "Remove this member from the workspace?"}
                              </p>
                              <div className={styles.accountActions}>
                                <button
                                  className={clsx(
                                    controls.buttonBase,
                                    controls.buttonSecondary,
                                  )}
                                  data-testid={`settings-permissions-member-remove-cancel-${memberId}`}
                                  onClick={() => setPendingRemoveMemberId(null)}
                                  type="button"
                                >
                                  Cancel
                                </button>
                                <button
                                  className={clsx(
                                    controls.buttonBase,
                                    controls.buttonDanger,
                                  )}
                                  data-testid={`settings-permissions-member-remove-confirm-action-${memberId}`}
                                  disabled={removingMemberId === memberId}
                                  onClick={() => {
                                    void handleRemoveMember(member);
                                  }}
                                  type="button"
                                >
                                  {removingMemberId === memberId
                                    ? "Removing…"
                                    : status === "invited"
                                      ? "Revoke invite"
                                      : "Remove member"}
                                </button>
                              </div>
                            </div>
                          ) : (
                            <button
                              className={clsx(
                                controls.buttonBase,
                                controls.buttonDanger,
                              )}
                              data-testid={`settings-permissions-member-remove-${memberId}`}
                              onClick={() => setPendingRemoveMemberId(memberId)}
                              type="button"
                            >
                              {status === "invited"
                                ? "Revoke invite"
                                : "Remove member"}
                            </button>
                          )}
                        </div>
                      </li>
                    );
                  })}
                </ul>
              ) : null}
            </div>
          </fieldset>
        ) : null}

        {activeTab === "appearance" ? (
          <fieldset
            className={styles.formSection}
            data-testid="settings-form-appearance"
          >
            <legend className={styles.legend}>Appearance</legend>
            <label className={controls.field}>
              Theme
              <select
                className={controls.selectInput}
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
            <label className={controls.field}>
              Density
              <select
                className={controls.selectInput}
                data-testid="settings-appearance-density"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    appearance: {
                      ...current.appearance,
                      density:
                        event.target.value === "compact"
                          ? "compact"
                          : event.target.value === "spacious"
                            ? "spacious"
                            : "comfortable",
                    },
                  }))
                }
                value={config.appearance.density}
              >
                <option value="compact">Compact</option>
                <option value="comfortable">Comfortable</option>
                <option value="spacious">Spacious</option>
              </select>
            </label>
            <label className={controls.field}>
              Base font size (px)
              <input
                className={controls.textInput}
                data-testid="settings-appearance-font-size"
                min={10}
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    appearance: {
                      ...current.appearance,
                      fontSize: asPositiveInt(
                        event.target.value,
                        current.appearance.fontSize,
                      ),
                    },
                  }))
                }
                type="number"
                value={config.appearance.fontSize}
              />
            </label>
            <label className={controls.field}>
              Editor font family
              <select
                className={controls.selectInput}
                data-testid="settings-editor-font-family"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    editor: {
                      ...current.editor,
                      fontFamily:
                        event.target.value === "sans"
                          ? "sans"
                          : event.target.value === "serif"
                            ? "serif"
                            : "mono",
                    },
                  }))
                }
                value={config.editor.fontFamily}
              >
                <option value="mono">Monospace</option>
                <option value="sans">Sans serif</option>
                <option value="serif">Serif</option>
              </select>
            </label>
            <label className={controls.field}>
              Editor tab size
              <input
                className={controls.textInput}
                data-testid="settings-editor-tab-size"
                max={8}
                min={1}
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    editor: {
                      ...current.editor,
                      tabSize: Math.min(
                        8,
                        Math.max(
                          1,
                          asPositiveInt(
                            event.target.value,
                            current.editor.tabSize,
                          ),
                        ),
                      ),
                    },
                  }))
                }
                type="number"
                value={config.editor.tabSize}
              />
            </label>
            <label className={controls.checkboxRow}>
              <input
                className={controls.checkbox}
                checked={config.editor.lineNumbers}
                data-testid="settings-editor-line-numbers"
                onChange={(event) =>
                  updateConfig((current) => ({
                    ...current,
                    editor: {
                      ...current.editor,
                      lineNumbers: event.target.checked,
                    },
                  }))
                }
                type="checkbox"
              />
              Show line numbers
            </label>
          </fieldset>
        ) : null}
      </div>
    </section>
  );
}
