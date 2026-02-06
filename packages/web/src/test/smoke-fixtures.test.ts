import { describe, expect, it } from "vitest";
import { createScriptumTestApi } from "./harness";
import { applySmokeFixture, SMOKE_FIXTURES } from "./smoke-fixtures";

describe("SMOKE_FIXTURES", () => {
  it("defines 5-10 uniquely named deterministic fixtures", () => {
    expect(SMOKE_FIXTURES.length).toBeGreaterThanOrEqual(5);
    expect(SMOKE_FIXTURES.length).toBeLessThanOrEqual(10);

    const ids = new Set(SMOKE_FIXTURES.map((fixture) => fixture.id));
    expect(ids.size).toBe(SMOKE_FIXTURES.length);
  });

  it("applies each fixture to the Scriptum test harness state", () => {
    for (const fixture of SMOKE_FIXTURES) {
      const api = createScriptumTestApi();
      applySmokeFixture(api, fixture);

      const state = api.getState();
      if (fixture.docContent !== undefined) {
        expect(state.docContent).toBe(fixture.docContent);
      }
      if (fixture.syncState !== undefined) {
        expect(state.syncState).toBe(fixture.syncState);
      }
      expect(state.fixtureName.length).toBeGreaterThan(0);
      expect(fixture.route.startsWith("/")).toBe(true);
    }
  });
});
