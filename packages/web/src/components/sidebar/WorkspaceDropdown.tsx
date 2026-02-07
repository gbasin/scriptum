import type { Workspace } from "@scriptum/shared";
import type { ChangeEvent } from "react";

const LAST_ACCESSED_FORMATTER = new Intl.DateTimeFormat("en-US", {
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  month: "short",
  timeZone: "UTC",
});

const WORKSPACE_DROPDOWN_ID = "workspace-switcher";

export interface WorkspaceDropdownProps {
  activeWorkspaceId: string | null;
  lastAccessedByWorkspaceId?: Record<string, string>;
  onCreateWorkspace?: () => void;
  onWorkspaceSelect?: (workspaceId: string) => void;
  workspaces: Workspace[];
}

export function formatLastAccessedLabel(
  lastAccessedAt: string | null | undefined,
): string {
  if (!lastAccessedAt) {
    return "Last accessed unknown";
  }

  const value = new Date(lastAccessedAt);
  if (Number.isNaN(value.getTime())) {
    return "Last accessed unknown";
  }

  return `Last accessed ${LAST_ACCESSED_FORMATTER.format(value)} UTC`;
}

export function WorkspaceDropdown({
  activeWorkspaceId,
  lastAccessedByWorkspaceId = {},
  onCreateWorkspace,
  onWorkspaceSelect,
  workspaces,
}: WorkspaceDropdownProps) {
  const handleWorkspaceChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const nextWorkspaceId = event.target.value;
    if (!nextWorkspaceId || !onWorkspaceSelect) {
      return;
    }
    onWorkspaceSelect(nextWorkspaceId);
  };

  return (
    <section aria-label="Workspace switcher" data-testid="workspace-switcher">
      <label
        htmlFor={WORKSPACE_DROPDOWN_ID}
        style={{
          display: "block",
          fontSize: "0.75rem",
          fontWeight: 600,
          letterSpacing: "0.04em",
          marginBottom: "0.25rem",
          textTransform: "uppercase",
        }}
      >
        Workspace
      </label>

      <div
        style={{
          alignItems: "center",
          display: "flex",
          gap: "0.5rem",
          marginBottom: "0.75rem",
        }}
      >
        <select
          aria-label="Workspace dropdown"
          data-testid="workspace-dropdown"
          id={WORKSPACE_DROPDOWN_ID}
          onChange={handleWorkspaceChange}
          style={{ flex: 1 }}
          value={activeWorkspaceId ?? ""}
        >
          {workspaces.length === 0 ? (
            <option value="">No workspaces yet</option>
          ) : (
            workspaces.map((workspace) => (
              <option key={workspace.id} value={workspace.id}>
                {workspace.name}
              </option>
            ))
          )}
        </select>

        <button
          aria-label="Create new workspace"
          data-testid="create-workspace-button"
          onClick={onCreateWorkspace}
          style={{ whiteSpace: "nowrap" }}
          type="button"
        >
          + New
        </button>
      </div>

      <ul
        aria-label="Workspace list"
        data-testid="workspace-last-accessed-list"
        style={{ listStyle: "none", margin: 0, padding: 0 }}
      >
        {workspaces.map((workspace) => {
          const lastAccessedAt =
            lastAccessedByWorkspaceId[workspace.id] ?? workspace.updatedAt;
          return (
            <li
              key={workspace.id}
              data-testid={`workspace-${workspace.id}`}
              style={{ marginBottom: "0.375rem" }}
            >
              <div style={{ fontWeight: 500 }}>{workspace.name}</div>
              <small>{formatLastAccessedLabel(lastAccessedAt)}</small>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
