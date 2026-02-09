import { renderToString } from "react-dom/server";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type MockAuthStatus = "unknown" | "authenticated" | "unauthenticated";
type MockRuntimeMode = "relay" | "local";

let mockStatus: MockAuthStatus = "unknown";
let mockMode: MockRuntimeMode = "relay";

vi.mock("../store/auth", () => ({
  useAuthStore: <T,>(selector: (state: { status: MockAuthStatus }) => T) =>
    selector({ status: mockStatus }),
}));

vi.mock("../test/setup", () => ({
  isFixtureModeEnabled: () => false,
}));

import { RequireAuth } from "./RequireAuth";

vi.mock("../store/runtime", () => ({
  useRuntimeStore: <T,>(
    selector: (state: { mode: MockRuntimeMode; modeResolved: boolean }) => T,
  ) =>
    selector({
      mode: mockMode,
      modeResolved: true,
    }),
}));

describe("RequireAuth", () => {
  beforeEach(() => {
    mockStatus = "unknown";
    mockMode = "relay";
  });

  afterEach(() => {
    mockMode = "relay";
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

  it("bypasses auth gating in local mode", () => {
    mockStatus = "unauthenticated";
    mockMode = "local";

    const html = renderToString(
      <MemoryRouter>
        <RequireAuth>
          <div>Protected content</div>
        </RequireAuth>
      </MemoryRouter>,
    );

    expect(html).toContain("Protected content");
  });
});
