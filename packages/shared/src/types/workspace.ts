export type WorkspaceTheme = "system" | "light" | "dark";

export type WorkspaceDensity = "comfortable" | "compact";

export type WorkspaceDefaultRole = "viewer" | "editor";

export interface WorkspaceGeneralConfig {
  workspaceName: string;
  defaultNewDocumentFolder: string;
  openLastDocumentOnLaunch: boolean;
}

export interface WorkspaceGitSyncConfig {
  enabled: boolean;
  autoCommitIntervalSeconds: number;
  commitMessageTemplate: string;
}

export interface WorkspaceAgentsConfig {
  allowAgentEdits: boolean;
  requireSectionLease: boolean;
  defaultAgentName: string;
}

export interface WorkspacePermissionsConfig {
  defaultRole: WorkspaceDefaultRole;
  allowExternalInvites: boolean;
  allowShareLinks: boolean;
}

export interface WorkspaceAppearanceConfig {
  theme: WorkspaceTheme;
  density: WorkspaceDensity;
  editorFontSizePx: number;
}

export interface WorkspaceConfig {
  general: WorkspaceGeneralConfig;
  gitSync: WorkspaceGitSyncConfig;
  agents: WorkspaceAgentsConfig;
  permissions: WorkspacePermissionsConfig;
  appearance: WorkspaceAppearanceConfig;
}

export interface Workspace {
  id: string;
  slug: string;
  name: string;
  role: string;
  createdAt: string;
  updatedAt: string;
  etag: string;
  config?: WorkspaceConfig;
}
