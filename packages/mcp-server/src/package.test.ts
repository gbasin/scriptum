import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

interface PackageJsonShape {
  readonly private?: boolean;
  readonly main?: string;
  readonly types?: string;
  readonly bin?: Record<string, string>;
}

describe("mcp package metadata", () => {
  it("defines scriptum-mcp binary metadata from the dist entrypoint", async () => {
    const packageJsonPath = new URL("../package.json", import.meta.url);
    const raw = await readFile(packageJsonPath, "utf8");
    const manifest = JSON.parse(raw) as PackageJsonShape;

    expect(manifest.private).toBe(true);
    expect(manifest.main).toBe("./dist/index.js");
    expect(manifest.types).toBe("./dist/index.d.ts");
    expect(manifest.bin).toEqual({
      "scriptum-mcp": "./dist/index.js",
    });
  });

  it("defines a node shebang in the CLI source entrypoint", async () => {
    const entrypointPath = new URL("./index.ts", import.meta.url);
    const source = await readFile(entrypointPath, "utf8");
    const firstLine = source.split(/\r?\n/u, 1)[0];
    expect(firstLine).toBe("#!/usr/bin/env node");
  });
});
