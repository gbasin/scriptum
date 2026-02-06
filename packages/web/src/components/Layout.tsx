import { Outlet } from "react-router-dom";

export function Layout() {
  return (
    <div data-testid="app-layout" style={{ display: "flex", minHeight: "100vh" }}>
      <aside
        data-testid="app-sidebar"
        style={{ borderRight: "1px solid #d1d5db", padding: "1rem", width: "18rem" }}
      >
        Sidebar
      </aside>
      <main data-testid="app-editor-area" style={{ flex: 1, padding: "1rem" }}>
        <Outlet />
      </main>
    </div>
  );
}

export const AppLayout = Layout;
