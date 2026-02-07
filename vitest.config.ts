import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    exclude: [
      ...configDefaults.exclude,
      "tests/e2e/specs/**/*.mts",
      "packages/web/tests/smoke/**/*.spec.ts",
    ],
  },
});
