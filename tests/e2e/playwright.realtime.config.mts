import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const baseURL = "http://127.0.0.1:4174";
const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);

export default defineConfig({
  testDir: "./specs",
  testMatch: "**/*.spec.mts",
  fullyParallel: true,
  timeout: 60_000,
  expect: {
    toHaveScreenshot: {
      maxDiffPixels: 0,
    },
  },
  reporter: "list",
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
  },
  webServer: {
    command:
      "pnpm --dir packages/web dev --host 127.0.0.1 --port 4174 --strictPort",
    cwd: repoRoot,
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      NODE_ENV: "test",
      VITE_SCRIPTUM_REALTIME_E2E: "1",
    },
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
      },
    },
  ],
});
