import { renderToString } from "react-dom/server";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";
import { NotFoundRoute } from "./not-found";

describe("NotFoundRoute", () => {
  it("renders a user-facing fallback message and home link", () => {
    const html = renderToString(
      <MemoryRouter>
        <NotFoundRoute />
      </MemoryRouter>,
    );

    expect(html).toContain('data-testid="not-found-route"');
    expect(html).toContain("Page not found");
    expect(html).toContain('data-testid="not-found-home-link"');
    expect(html).toContain('href="/"');
  });
});
