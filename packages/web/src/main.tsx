import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { installScriptumTestApi } from "./test/harness";
import { setupFixtureMode } from "./test/setup";

setupFixtureMode();
installScriptumTestApi();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
