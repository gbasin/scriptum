import { Outlet } from "react-router-dom";

export function Layout() {
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
        <h2>Sidebar</h2>
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
