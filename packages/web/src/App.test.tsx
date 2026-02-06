import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MemoryRouter } from "react-router-dom";

import { AppRoutes } from "./App";

function renderAt(path: string): string {
  return renderToString(
    <MemoryRouter initialEntries={[path]}>
      <AppRoutes />
    </MemoryRouter>
  );
}

describe("AppRoutes", () => {
  it("renders index route", () => {
    const html = renderAt("/");
    expect(html).toContain("Workspace Selector");
  });

  it("renders workspace route in app layout", () => {
    const html = renderAt("/workspace/demo");
    expect(html).toContain("Sidebar");
    expect(html).toContain("Workspace:");
    expect(html).toContain("demo</section>");
  });

  it("renders document route in app layout", () => {
    const html = renderAt("/workspace/demo/document/readme");
    expect(html).toContain("Document:");
    expect(html).toContain("demo");
    expect(html).toContain("readme</section>");
  });

  it("renders settings route in app layout", () => {
    const html = renderAt("/settings");
    expect(html).toContain("Settings");
    expect(html).toContain("Sidebar");
  });

  it("renders auth callback route", () => {
    const html = renderAt("/auth-callback");
    expect(html).toContain("Auth Callback");
  });
});
