import type { Workspace } from "@scriptum/shared";
import clsx from "clsx";
import type { ChangeEvent } from "react";
import controls from "../../styles/Controls.module.css";
import styles from "./WorkspaceDropdown.module.css";

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
      <label className={styles.label} htmlFor={WORKSPACE_DROPDOWN_ID}>
        Workspace
      </label>

      <div className={styles.controlsRow}>
        <select
          aria-label="Workspace dropdown"
          className={clsx(controls.selectInput, styles.dropdown)}
          data-testid="workspace-dropdown"
          id={WORKSPACE_DROPDOWN_ID}
          onChange={handleWorkspaceChange}
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
          className={clsx(
            controls.buttonBase,
            controls.buttonSecondary,
            styles.createButton,
          )}
          data-testid="create-workspace-button"
          onClick={onCreateWorkspace}
          type="button"
        >
          + New
        </button>
      </div>

      <ul
        aria-label="Workspace list"
        className={styles.workspaceList}
        data-testid="workspace-last-accessed-list"
      >
        {workspaces.map((workspace) => {
          const lastAccessedAt =
            lastAccessedByWorkspaceId[workspace.id] ?? workspace.updatedAt;
          return (
            <li
              className={styles.workspaceListItem}
              key={workspace.id}
              data-testid={`workspace-${workspace.id}`}
            >
              <div className={styles.workspaceName}>{workspace.name}</div>
              <small className={styles.workspaceTimestamp}>
                {formatLastAccessedLabel(lastAccessedAt)}
              </small>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
