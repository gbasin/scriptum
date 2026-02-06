import { createBrowserRouter, type RouteObject } from "react-router-dom";
import { Layout } from "./components/Layout";
import { RequireAuth } from "./components/RequireAuth";
import { AuthCallbackRoute } from "./routes/auth-callback";
import { DocumentRoute } from "./routes/document";
import { IndexRoute } from "./routes/index";
import { SettingsRoute } from "./routes/settings";
import { WorkspaceRoute } from "./routes/workspace";

export const appRoutes: RouteObject[] = [
  { id: "index", path: "/", element: <IndexRoute /> },
  {
    id: "app-layout",
    element: (
      <RequireAuth>
        <Layout />
      </RequireAuth>
    ),
    children: [
      {
        id: "workspace",
        path: "workspace/:workspaceId",
        element: <WorkspaceRoute />,
      },
      {
        id: "document",
        path: "workspace/:workspaceId/document/:documentId",
        element: <DocumentRoute />,
      },
      { id: "settings", path: "settings", element: <SettingsRoute /> },
    ],
  },
  { id: "auth-callback", path: "/auth-callback", element: <AuthCallbackRoute /> },
];

export function createAppRouter() {
  return createBrowserRouter(appRoutes);
}
