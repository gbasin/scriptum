import { matchRoutes } from "react-router-dom";
import { describe, expect, it } from "vitest";
import { appRoutes } from "./router";

function matchedRouteIds(pathname: string): string[] {
  const matches = matchRoutes(appRoutes, pathname);
  if (!matches) {
    return [];
  }

  return matches.map((match) => String(match.route.id));
}

describe("appRoutes", () => {
  it("matches the required top-level routes", () => {
    expect(matchedRouteIds("/")).toEqual(["index"]);
    expect(matchedRouteIds("/auth-callback")).toEqual(["auth-callback"]);
  });

  it("routes workspace, document, and settings through the app layout", () => {
    expect(matchedRouteIds("/workspace/ws-123")).toEqual([
      "app-layout",
      "workspace",
    ]);
    expect(
      matchedRouteIds("/workspace/ws-123/document/doc-456")
    ).toEqual(["app-layout", "document"]);
    expect(matchedRouteIds("/settings")).toEqual(["app-layout", "settings"]);
  });

  it("returns no match for unknown paths", () => {
    expect(matchedRouteIds("/missing")).toEqual([]);
  });
});
