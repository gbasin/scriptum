// Workspace roles — derived from contracts/roles.json.

export const WORKSPACE_ROLES = ["viewer", "editor", "owner"] as const;

export type WorkspaceRole = (typeof WORKSPACE_ROLES)[number];

export const DEFAULT_ASSIGNABLE_ROLES = ["viewer", "editor"] as const;

export type WorkspaceDefaultRole = (typeof DEFAULT_ASSIGNABLE_ROLES)[number];
