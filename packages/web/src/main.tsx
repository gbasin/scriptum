import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import {
  startAppearanceSync,
  startGitSyncPollingSync,
  startThemeSync,
} from "./lib/theme";
import { useWorkspaceStore } from "./store/workspace";
import "./styles/tokens.css";
import "./styles/base.css";
import { installScriptumTestApi } from "./test/harness";
import { setupFixtureMode } from "./test/setup";

setupFixtureMode();
installScriptumTestApi();
startThemeSync(useWorkspaceStore);
startAppearanceSync(useWorkspaceStore);
startGitSyncPollingSync(useWorkspaceStore);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
