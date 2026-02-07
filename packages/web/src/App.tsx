import { BrowserRouter, useRoutes } from "react-router-dom";
import { ErrorBoundary } from "./components/ErrorBoundary";
import styles from "./App.module.css";
import { appRoutes } from "./router";

export function AppRoutes() {
  return useRoutes(appRoutes);
}

export function App() {
  return (
    <div className={styles.appShell}>
      <ErrorBoundary>
        <BrowserRouter>
          <AppRoutes />
        </BrowserRouter>
      </ErrorBoundary>
    </div>
  );
}
