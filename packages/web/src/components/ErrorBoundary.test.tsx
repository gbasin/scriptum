// @vitest-environment jsdom

import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function renderBoundary(ui: ReactNode) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(ui);
  });

  return { container, root };
}

function ThrowOnRender(): ReactNode {
  throw new Error("boom");
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  vi.restoreAllMocks();
});

describe("ErrorBoundary", () => {
  it("renders children when no error is thrown", () => {
    const { container, root } = renderBoundary(
      <ErrorBoundary>
        <div data-testid="healthy-view">ok</div>
      </ErrorBoundary>,
    );

    expect(
      container.querySelector('[data-testid="healthy-view"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="app-error-boundary"]'),
    ).toBeNull();

    act(() => {
      root.unmount();
    });
  });

  it("shows fallback UI and triggers reload callback after a render crash", () => {
    const onReload = vi.fn();
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);

    const { container, root } = renderBoundary(
      <ErrorBoundary onReload={onReload}>
        <ThrowOnRender />
      </ErrorBoundary>,
    );

    expect(
      container.querySelector('[data-testid="app-error-boundary"]'),
    ).not.toBeNull();
    const reloadButton = container.querySelector(
      '[data-testid="app-error-boundary-reload"]',
    ) as HTMLButtonElement | null;

    act(() => {
      reloadButton?.click();
    });

    expect(onReload).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalled();

    act(() => {
      root.unmount();
    });
  });

  it("supports inline fallback copy overrides", () => {
    const onReload = vi.fn();
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);

    const { container, root } = renderBoundary(
      <ErrorBoundary
        inline
        message="Editor failed but the rest of the view is still available."
        onReload={onReload}
        reloadLabel="Retry editor"
        testId="editor-error-boundary"
        title="Editor failed to load"
      >
        <ThrowOnRender />
      </ErrorBoundary>,
    );

    expect(
      container.querySelector('[data-testid="editor-error-boundary"]'),
    ).not.toBeNull();
    expect(container.textContent).toContain("Editor failed to load");
    expect(container.textContent).toContain(
      "Editor failed but the rest of the view is still available.",
    );

    const reloadButton = container.querySelector(
      '[data-testid="editor-error-boundary-reload"]',
    ) as HTMLButtonElement | null;
    expect(reloadButton?.textContent).toBe("Retry editor");

    act(() => {
      reloadButton?.click();
    });

    expect(onReload).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalled();

    act(() => {
      root.unmount();
    });
  });
});
