import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const baseURL = "http://127.0.0.1:4173";
const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);

export default defineConfig({
  testDir: "./specs",
  testMatch: "**/*.spec.mts",
  fullyParallel: true,
  timeout: 30_000,
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
      "pnpm --dir packages/web dev --host 127.0.0.1 --port 4173 --strictPort",
    cwd: repoRoot,
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      VITE_SCRIPTUM_FIXTURE_MODE: "1",
      NODE_ENV: "test",
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
