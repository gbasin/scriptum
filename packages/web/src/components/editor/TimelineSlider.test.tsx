// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clampTimelineValue, TimelineSlider } from "./TimelineSlider";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

describe("TimelineSlider", () => {
  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  });

  it("renders timeline slider metadata for the current version window", () => {
    const html = renderToString(
      <TimelineSlider
        max={5}
        onChange={() => {
          // no-op for server-side render test
        }}
        onViewModeChange={() => {
          // no-op for server-side render test
        }}
        value={2}
        viewMode="authorship"
      />,
    );
    const normalized = html.replaceAll("<!-- -->", "");

    expect(normalized).toContain("History timeline");
    expect(normalized).toContain("Colored authorship");
    expect(normalized).toContain("Diff from current");
    expect(normalized).toContain('type="range"');
    expect(normalized).toContain('max="5"');
    expect(normalized).toContain('value="2"');
    expect(normalized).toContain("Version 3/6");
  });

  it("clamps timeline values into [0, max]", () => {
    expect(clampTimelineValue(-5, 10)).toBe(0);
    expect(clampTimelineValue(3, 10)).toBe(3);
    expect(clampTimelineValue(99, 10)).toBe(10);
    expect(clampTimelineValue(Number.NaN, 10)).toBe(0);
  });

  it("calls onViewModeChange when toggling between authorship and diff modes", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const onViewModeChange = vi.fn();

    act(() => {
      root.render(
        <TimelineSlider
          max={4}
          onChange={() => {
            // no-op
          }}
          onViewModeChange={onViewModeChange}
          value={1}
          viewMode="authorship"
        />,
      );
    });

    const diffButton = container.querySelector(
      '[data-testid="history-view-toggle-diff"]',
    ) as HTMLButtonElement | null;
    expect(diffButton).not.toBeNull();
    act(() => {
      diffButton?.click();
    });
    expect(onViewModeChange).toHaveBeenCalledWith("diff");

    act(() => {
      root.render(
        <TimelineSlider
          max={4}
          onChange={() => {
            // no-op
          }}
          onViewModeChange={onViewModeChange}
          value={1}
          viewMode="diff"
        />,
      );
    });

    const modeLabel = container.querySelector(
      '[data-testid="history-view-mode-label"]',
    );
    expect(modeLabel?.textContent).toContain("Diff from current");

    act(() => {
      root.unmount();
    });
  });
});
