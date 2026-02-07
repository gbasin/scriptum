import { renderToString } from "react-dom/server";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

type MockAuthStatus = "unknown" | "authenticated" | "unauthenticated";

let mockStatus: MockAuthStatus = "unknown";

vi.mock("../store/auth", () => ({
  useAuthStore: <T,>(selector: (state: { status: MockAuthStatus }) => T) =>
    selector({ status: mockStatus }),
}));

vi.mock("../test/setup", () => ({
  isFixtureModeEnabled: () => false,
}));

import { RequireAuth } from "./RequireAuth";

describe("RequireAuth", () => {
  beforeEach(() => {
    mockStatus = "unknown";
  });

  it("renders a skeleton while auth state is unknown", () => {
    const html = renderToString(
      <MemoryRouter>
        <RequireAuth>
          <div>Protected content</div>
        </RequireAuth>
      </MemoryRouter>,
    );

    expect(html).toContain('data-testid="require-auth-skeleton"');
    expect(html).not.toContain("Protected content");
  });

  it("renders protected content once authenticated", () => {
    mockStatus = "authenticated";

    const html = renderToString(
      <MemoryRouter>
        <RequireAuth>
          <div>Protected content</div>
        </RequireAuth>
      </MemoryRouter>,
    );

    expect(html).toContain("Protected content");
    expect(html).not.toContain('data-testid="require-auth-skeleton"');
  });
});
