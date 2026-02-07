// @vitest-environment jsdom

import type { ComponentProps } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Outline } from "./Outline";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function flushMicrotasks(): Promise<void> {
  return Promise.resolve();
}

function createEditorContainer(markup: string): HTMLElement {
  const container = document.createElement("article");
  container.innerHTML = markup;
  document.body.appendChild(container);
  return container;
}

function renderOutline(props: ComponentProps<typeof Outline>) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  const render = (nextProps: ComponentProps<typeof Outline>) => {
    act(() => {
      root.render(<Outline {...nextProps} />);
    });
  };

  render(props);

  return {
    container,
    rerender: (nextProps: ComponentProps<typeof Outline>) => render(nextProps),
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

describe("Outline", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  });

  it("renders hierarchical headings and scrolls to selected heading", () => {
    const editorContainer = createEditorContainer(
      "<h1>Summary</h1><h2>Overview</h2><h3>Implementation details</h3>",
    );
    const harness = renderOutline({ editorContainer });

    const list = harness.container.querySelector(
      '[data-testid="outline-list"]',
    );
    expect(list).not.toBeNull();
    expect(harness.container.textContent).toContain("Summary");
    expect(harness.container.textContent).toContain("Overview");
    expect(harness.container.textContent).toContain("Implementation details");

    const implementationHeading = editorContainer.querySelectorAll(
      "h1,h2,h3",
    )[2] as HTMLElement | undefined;
    expect(implementationHeading).toBeDefined();
    const scrollIntoViewSpy = vi.fn();
    if (implementationHeading) {
      implementationHeading.scrollIntoView = scrollIntoViewSpy;
    }
    const button = Array.from(
      harness.container.querySelectorAll<HTMLButtonElement>(
        '[data-testid^="outline-heading-"]',
      ),
    ).find((candidate) =>
      candidate.textContent?.includes("Implementation details"),
    );

    expect(button).toBeDefined();

    act(() => {
      button?.click();
    });
    expect(scrollIntoViewSpy).toHaveBeenCalledTimes(1);

    harness.unmount();
  });

  it("highlights the currently visible heading on scroll", () => {
    const editorContainer = createEditorContainer(
      "<h1>Summary</h1><h2>Overview</h2><h2>Implementation</h2>",
    );
    const headings = Array.from(
      editorContainer.querySelectorAll<HTMLElement>("h1,h2"),
    );
    const [summaryHeading, overviewHeading, implementationHeading] = headings;

    summaryHeading.getBoundingClientRect = () =>
      ({
        bottom: -120,
        height: 20,
        left: 0,
        right: 200,
        top: -140,
        width: 200,
      }) as DOMRect;
    overviewHeading.getBoundingClientRect = () =>
      ({
        bottom: 70,
        height: 20,
        left: 0,
        right: 200,
        top: 50,
        width: 200,
      }) as DOMRect;
    implementationHeading.getBoundingClientRect = () =>
      ({
        bottom: 290,
        height: 20,
        left: 0,
        right: 200,
        top: 270,
        width: 200,
      }) as DOMRect;

    const harness = renderOutline({ editorContainer });

    act(() => {
      window.dispatchEvent(new Event("scroll"));
    });

    const active = harness.container.querySelector(
      '[data-active="true"]',
    ) as HTMLButtonElement | null;
    expect(active?.textContent).toContain("Overview");

    harness.unmount();
  });

  it("updates headings in real-time and truncates long labels", async () => {
    const editorContainer = createEditorContainer("<h1>Summary</h1>");
    const harness = renderOutline({ editorContainer });

    const longHeading = document.createElement("h2");
    longHeading.textContent =
      "This heading is intentionally very long to verify truncation behavior";

    act(() => {
      editorContainer.appendChild(longHeading);
    });
    await act(async () => {
      await flushMicrotasks();
    });

    const headingButton = Array.from(
      harness.container.querySelectorAll<HTMLButtonElement>(
        '[data-testid^="outline-heading-"]',
      ),
    ).find((button) => button.textContent?.includes("intentionally very long"));

    expect(headingButton).toBeDefined();
    expect(headingButton?.title).toContain("intentionally very long");

    harness.unmount();
  });

  it("renders loading skeleton state", () => {
    const harness = renderOutline({
      editorContainer: null,
      loading: true,
    });

    expect(
      harness.container.querySelector('[data-testid="outline-loading"]'),
    ).not.toBeNull();
    expect(
      harness.container.querySelector('[data-testid="outline-empty"]'),
    ).toBeNull();

    harness.unmount();
  });
});
