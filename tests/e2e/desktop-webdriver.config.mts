import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./specs",
  testMatch: "**/desktop-tauri-webdriver.spec.mts",
  fullyParallel: false,
  workers: 1,
  timeout: 90_000,
  reporter: "list",
});
