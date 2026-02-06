import type { Workspace } from "@scriptum/shared";
import { Outlet } from "react-router-dom";
import { useWorkspaceStore } from "../store/workspace";
import { WorkspaceDropdown } from "./sidebar/WorkspaceDropdown";

export function Layout() {
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId);
  const setActiveWorkspaceId = useWorkspaceStore(
    (state) => state.setActiveWorkspaceId
  );
  const upsertWorkspace = useWorkspaceStore((state) => state.upsertWorkspace);
  const workspaces = useWorkspaceStore((state) => state.workspaces);

  const handleCreateWorkspace = () => {
    const token = Date.now().toString(36);
    const now = new Date().toISOString();
    const workspaceId = `ws-${token}`;
    const workspace: Workspace = {
      id: workspaceId,
      slug: workspaceId,
      name: `Workspace ${workspaces.length + 1}`,
      role: "owner",
      createdAt: now,
      updatedAt: now,
      etag: `workspace-${token}`,
    };

    upsertWorkspace(workspace);
    setActiveWorkspaceId(workspace.id);
  };

  return (
    <div
      data-testid="app-layout"
      style={{ display: "flex", minHeight: "100vh" }}
    >
      <aside
        aria-label="Sidebar"
        data-testid="app-sidebar"
        style={{
          borderRight: "1px solid #d1d5db",
          padding: "1rem",
          width: "18rem",
        }}
      >
        <WorkspaceDropdown
          activeWorkspaceId={activeWorkspaceId}
          onCreateWorkspace={handleCreateWorkspace}
          onWorkspaceSelect={setActiveWorkspaceId}
          workspaces={workspaces}
        />
        <h2 style={{ marginBottom: "0.25rem", marginTop: "1rem" }}>Sidebar</h2>
        <p>Navigation and context panels.</p>
      </aside>
      <main
        aria-label="Editor area"
        data-testid="app-editor-area"
        style={{ flex: 1, padding: "1rem" }}
      >
        <Outlet />
      </main>
    </div>
  );
}

export const AppLayout = Layout;
