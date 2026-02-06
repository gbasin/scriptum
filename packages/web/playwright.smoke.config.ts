import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/smoke",
  timeout: 30_000,
  expect: {
    timeout: 10_000,
  },
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    browserName: "chromium",
    viewport: { width: 1280, height: 800 },
    baseURL: "http://127.0.0.1:4173",
    colorScheme: "light",
  },
  webServer: {
    command: "pnpm --filter @scriptum/web dev --host 127.0.0.1 --port 4173",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: true,
    timeout: 120_000,
    env: {
      VITE_SCRIPTUM_FIXTURE_MODE: "1",
    },
  },
});
