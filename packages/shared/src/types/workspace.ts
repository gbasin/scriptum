import type { WorkspaceDefaultRole } from "../contracts/roles";

export type WorkspaceTheme = "system" | "light" | "dark";

export type WorkspaceDensity = "compact" | "comfortable" | "spacious";

export type WorkspaceEditorFontFamily = "mono" | "sans" | "serif";

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
  fontSize: number;
}

export interface WorkspaceEditorConfig {
  fontFamily: WorkspaceEditorFontFamily;
  tabSize: number;
  lineNumbers: boolean;
}

export interface WorkspaceConfig {
  general: WorkspaceGeneralConfig;
  gitSync: WorkspaceGitSyncConfig;
  agents: WorkspaceAgentsConfig;
  permissions: WorkspacePermissionsConfig;
  appearance: WorkspaceAppearanceConfig;
  editor: WorkspaceEditorConfig;
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
