import { BrowserRouter, useRoutes } from "react-router-dom";
import { appRoutes } from "./router";

export function AppRoutes() {
  return useRoutes(appRoutes);
}

export function App() {
  return (
    <BrowserRouter>
      <AppRoutes />
    </BrowserRouter>
  );
}
