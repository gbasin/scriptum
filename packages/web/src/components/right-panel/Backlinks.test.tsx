// @vitest-environment jsdom

import { act } from "react";
import type { ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  Backlinks,
  normalizeBacklinksResponse,
  type BacklinkEntry,
} from "./Backlinks";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function flushMicrotasks(): Promise<void> {
  return Promise.resolve();
}

function renderBacklinks(props: ComponentProps<typeof Backlinks>) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  const render = (nextProps: ComponentProps<typeof Backlinks>) => {
    act(() => {
      root.render(<Backlinks {...nextProps} />);
    });
  };

  render(props);

  return {
    container,
    rerender: (nextProps: ComponentProps<typeof Backlinks>) => {
      render(nextProps);
    },
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

describe("normalizeBacklinksResponse", () => {
  it("normalizes backlinks from both top-level and context payload shapes", () => {
    expect(
      normalizeBacklinksResponse({
        context: {
          backlinks: [
            {
              doc_id: "doc-1",
              link_text: "Auth Flow",
              path: "docs/auth.md",
              snippet: "See [[Auth Flow]] for details.",
              title: "Auth",
            },
          ],
        },
      }),
    ).toEqual<BacklinkEntry[]>([
      {
        docId: "doc-1",
        linkText: "[[Auth Flow]]",
        path: "docs/auth.md",
        snippet: "See [[Auth Flow]] for details.",
        title: "Auth",
      },
    ]);
  });
});

describe("Backlinks", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.restoreAllMocks();
  });

  it("renders empty state when there are no backlinks", async () => {
    const fetchBacklinks = vi
      .fn<(workspaceId: string, documentId: string) => Promise<BacklinkEntry[]>>()
      .mockResolvedValue([]);

    const harness = renderBacklinks({
      documentId: "doc-main",
      fetchBacklinks,
      workspaceId: "ws-main",
    });

    await act(async () => {
      await flushMicrotasks();
    });

    expect(fetchBacklinks).toHaveBeenCalledWith("ws-main", "doc-main");
    expect(harness.container.querySelector("[data-testid=\"backlinks-empty\"]")?.textContent)
      .toContain("No documents link to this page.");

    harness.unmount();
  });

  it("renders backlinks and notifies selection callback", async () => {
    const fetchBacklinks = vi
      .fn<(workspaceId: string, documentId: string) => Promise<BacklinkEntry[]>>()
      .mockResolvedValue([
        {
          docId: "doc-source",
          linkText: "[[Auth]]",
          path: "docs/overview.md",
          snippet: "Reference to [[Auth]] section.",
          title: "Overview",
        },
      ]);
    const onBacklinkSelect = vi.fn<(documentId: string) => void>();

    const harness = renderBacklinks({
      documentId: "doc-main",
      fetchBacklinks,
      onBacklinkSelect,
      workspaceId: "ws-main",
    });

    await act(async () => {
      await flushMicrotasks();
    });

    expect(harness.container.textContent).toContain("Overview");
    expect(harness.container.textContent).toContain("docs/overview.md");
    expect(harness.container.textContent).toContain("[[Auth]]");
    expect(harness.container.textContent).toContain("Reference to [[Auth]] section.");

    const button = harness.container.querySelector(
      "[data-testid=\"backlink-item-doc-source\"]",
    ) as HTMLButtonElement | null;
    expect(button).not.toBeNull();

    act(() => {
      button?.click();
    });
    expect(onBacklinkSelect).toHaveBeenCalledWith("doc-source");

    harness.unmount();
  });

  it("refetches backlinks when refresh token changes", async () => {
    const fetchBacklinks = vi
      .fn<(workspaceId: string, documentId: string) => Promise<BacklinkEntry[]>>()
      .mockResolvedValue([]);

    const harness = renderBacklinks({
      documentId: "doc-main",
      fetchBacklinks,
      refreshToken: 1,
      workspaceId: "ws-main",
    });

    await act(async () => {
      await flushMicrotasks();
    });
    expect(fetchBacklinks).toHaveBeenCalledTimes(1);

    harness.rerender({
      documentId: "doc-main",
      fetchBacklinks,
      refreshToken: 2,
      workspaceId: "ws-main",
    });
    await act(async () => {
      await flushMicrotasks();
    });

    expect(fetchBacklinks).toHaveBeenCalledTimes(2);
    harness.unmount();
  });
});
