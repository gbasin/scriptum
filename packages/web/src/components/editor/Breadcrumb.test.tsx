// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  Breadcrumb,
  buildBreadcrumbSegments,
  truncateBreadcrumbLabel,
} from "./Breadcrumb";

let container: HTMLDivElement | null = null;
let root: Root | null = null;

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(() => {
  if (root) {
    act(() => {
      root?.unmount();
    });
  }
  root = null;
  container?.remove();
  container = null;
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

function renderBreadcrumb(props: {
  path: string;
  workspaceLabel: string;
  onNavigate?: (path: string | null) => void;
}): HTMLDivElement {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);

  act(() => {
    root?.render(<Breadcrumb {...props} />);
  });

  return container;
}

describe("Breadcrumb", () => {
  it("renders workspace name and document path segments", () => {
    const view = renderBreadcrumb({
      path: "docs/guides/start.md",
      workspaceLabel: "Workspace Alpha",
    });

    expect(
      view.querySelector('[data-testid="breadcrumb-root"]')?.textContent,
    ).toBe("Workspace Alpha");
    expect(
      view.querySelector('[data-testid="breadcrumb-docs"]')?.textContent,
    ).toContain("docs");
    expect(
      view.querySelector('[data-testid="breadcrumb-docs/guides"]')?.textContent,
    ).toContain("guides");
    expect(
      view.querySelector('[data-testid="breadcrumb-docs/guides/start.md"]')
        ?.textContent,
    ).toContain("start.md");
  });

  it("renders section breadcrumb when inside a section path", () => {
    const view = renderBreadcrumb({
      path: "docs/guides/start.md/section/architecture",
      workspaceLabel: "Workspace Alpha",
    });

    expect(
      view.querySelector(
        '[data-testid="breadcrumb-docs/guides/start.md/section/architecture"]',
      )?.textContent,
    ).toContain("architecture");
  });

  it("navigates to clicked breadcrumb segment", () => {
    const onNavigate = vi.fn();
    const view = renderBreadcrumb({
      path: "docs/guides/start.md",
      workspaceLabel: "Workspace Alpha",
      onNavigate,
    });

    const rootButton = view.querySelector(
      '[data-testid="breadcrumb-root"]',
    ) as HTMLButtonElement | null;
    const middleSegment = view.querySelector(
      '[data-testid="breadcrumb-segment-docs/guides"]',
    ) as HTMLButtonElement | null;

    act(() => {
      rootButton?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      middleSegment?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(onNavigate).toHaveBeenNthCalledWith(1, null);
    expect(onNavigate).toHaveBeenNthCalledWith(2, "docs/guides");
  });

  it("truncates long labels while keeping full text in title", () => {
    const longWorkspaceLabel =
      "Workspace Label That Is Definitely Longer Than Twenty Four";
    const longSegment = "very-long-segment-name-that-needs-truncation.md";

    const view = renderBreadcrumb({
      path: `docs/${longSegment}`,
      workspaceLabel: longWorkspaceLabel,
    });

    const rootButton = view.querySelector(
      '[data-testid="breadcrumb-root"]',
    ) as HTMLButtonElement | null;
    const segmentButton = view.querySelector(
      `[data-testid="breadcrumb-segment-docs/${longSegment}"]`,
    ) as HTMLButtonElement | null;

    expect(rootButton?.textContent).toBe(
      truncateBreadcrumbLabel(longWorkspaceLabel),
    );
    expect(rootButton?.title).toBe(longWorkspaceLabel);
    expect(segmentButton?.textContent).toBe(
      truncateBreadcrumbLabel(longSegment),
    );
    expect(segmentButton?.title).toBe(longSegment);
    expect(segmentButton?.textContent?.endsWith("…")).toBe(true);
  });
});

describe("buildBreadcrumbSegments", () => {
  it("builds cumulative paths from slash-delimited input", () => {
    expect(buildBreadcrumbSegments("docs/guides/start.md")).toEqual([
      { label: "docs", path: "docs" },
      { label: "guides", path: "docs/guides" },
      { label: "start.md", path: "docs/guides/start.md" },
    ]);
  });
});
