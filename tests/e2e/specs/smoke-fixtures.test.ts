import { describe, expect, it } from "vitest";
import { loadSmokeFixtures } from "./smoke-fixtures.mts";

describe("loadSmokeFixtures", () => {
  it("loads 5-10 fixtures with required UI coverage", () => {
    const fixtures = loadSmokeFixtures();
    const syncStates = new Set(
      fixtures
        .map((fixture) => fixture.expectations.syncState)
        .filter(
          (state): state is NonNullable<typeof state> => state !== undefined,
        ),
    );

    expect(fixtures.length).toBeGreaterThanOrEqual(5);
    expect(fixtures.length).toBeLessThanOrEqual(10);
    expect(
      fixtures.some(
        (fixture) =>
          fixture.route.startsWith("/workspace/") &&
          fixture.route.includes("/document/"),
      ),
    ).toBe(true);
    expect(
      fixtures.some(
        (fixture) =>
          fixture.route.startsWith("/workspace/") &&
          !fixture.route.includes("/document/"),
      ),
    ).toBe(true);
    expect(fixtures.some((fixture) => fixture.route === "/settings")).toBe(
      true,
    );
    expect(fixtures.some((fixture) => fixture.route === "/auth-callback")).toBe(
      true,
    );

    expect(syncStates.has("synced")).toBe(true);
    expect(syncStates.has("offline")).toBe(true);
    expect(syncStates.has("reconnecting")).toBe(true);
    expect(syncStates.has("error")).toBe(true);
    expect(
      fixtures.some(
        (fixture) =>
          fixture.state?.syncState === "offline" &&
          (fixture.state.pendingSyncUpdates ?? 0) > 0,
      ),
    ).toBe(true);
    expect(
      fixtures.some(
        (fixture) =>
          fixture.state?.syncState === "reconnecting" &&
          (fixture.state.reconnectProgress?.totalUpdates ?? 0) > 0,
      ),
    ).toBe(true);
    expect(
      fixtures.some(
        (fixture) => typeof fixture.state?.gitStatus?.lastCommit === "string",
      ),
    ).toBe(true);
  });
});
