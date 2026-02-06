import { describe, expect, it } from "vitest";
import { loadSmokeFixtures } from "./smoke-fixtures.mts";

describe("loadSmokeFixtures", () => {
  it("loads 5-10 fixtures with coverage across editor/sidebar/presence/sync", () => {
    const fixtures = loadSmokeFixtures();
    const names = fixtures.map((fixture) => fixture.name);

    expect(fixtures.length).toBeGreaterThanOrEqual(5);
    expect(fixtures.length).toBeLessThanOrEqual(10);
    expect(names.some((name) => name.includes("workspace"))).toBe(true);
    expect(names.some((name) => name.includes("editor"))).toBe(true);
    expect(names.some((name) => name.includes("presence"))).toBe(true);
    expect(names.some((name) => name.includes("sync"))).toBe(true);
  });
});
