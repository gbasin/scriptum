// @vitest-environment jsdom

import { nameToColor } from "@scriptum/editor";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CursorLabel, type CursorLabelPeer } from "./CursorLabel";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makePeer(overrides: Partial<CursorLabelPeer> = {}): CursorLabelPeer {
  return {
    color: undefined,
    cursorPosition: { x: 120, y: 48 },
    name: "Remote Agent",
    type: "agent",
    ...overrides,
  };
}

function hexToRgb(hex: string): string {
  const normalized = hex.replace("#", "");
  const red = Number.parseInt(normalized.slice(0, 2), 16);
  const green = Number.parseInt(normalized.slice(2, 4), 16);
  const blue = Number.parseInt(normalized.slice(4, 6), 16);
  return `rgb(${red}, ${green}, ${blue})`;
}

function renderCursorLabel(peer: CursorLabelPeer, autoHideMs = 3_000) {
  const container = document.createElement("div");
  container.style.position = "relative";
  document.body.appendChild(container);
  const root = createRoot(container);

  const render = (nextPeer: CursorLabelPeer) => {
    act(() => {
      root.render(<CursorLabel autoHideMs={autoHideMs} peer={nextPeer} />);
    });
  };

  render(peer);

  return {
    queryLabel: () =>
      container.querySelector(
        '[data-testid="cursor-label"]',
      ) as HTMLElement | null,
    render,
    unmount: () => {
      act(() => {
        root.unmount();
      });
      container.remove();
    },
  };
}

describe("CursorLabel", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.useRealTimers();
  });

  it("renders nothing when the peer has no cursor position", () => {
    const harness = renderCursorLabel(makePeer({ cursorPosition: null }));
    expect(harness.queryLabel()).toBeNull();
    harness.unmount();
  });

  it("renders an agent label above the cursor with deterministic color", () => {
    const peer = makePeer({
      color: undefined,
      name: "Scriptum Bot",
      type: "agent",
    });
    const harness = renderCursorLabel(peer);

    const label = harness.queryLabel();
    expect(label).not.toBeNull();
    expect(label?.textContent).toContain("Scriptum Bot");
    expect(
      label?.querySelector('[data-testid="cursor-label-agent-icon"]'),
    ).not.toBeNull();
    expect(label?.style.left).toBe("120px");
    expect(label?.style.top).toBe("48px");
    expect(label?.style.transform).toContain("translate(-50%");
    expect(label?.style.backgroundColor).toBe(
      hexToRgb(nameToColor("Scriptum Bot")),
    );

    harness.unmount();
  });

  it("does not render the agent icon for human peers", () => {
    const harness = renderCursorLabel(
      makePeer({ name: "Alice", type: "human" }),
    );
    const label = harness.queryLabel();
    expect(label).not.toBeNull();
    expect(
      label?.querySelector('[data-testid="cursor-label-agent-icon"]'),
    ).toBeNull();
    harness.unmount();
  });

  it("auto-hides after cursor inactivity", () => {
    vi.useFakeTimers();
    const harness = renderCursorLabel(makePeer(), 3_000);
    expect(harness.queryLabel()).not.toBeNull();

    act(() => {
      vi.advanceTimersByTime(3_001);
    });

    expect(harness.queryLabel()).toBeNull();
    harness.unmount();
  });

  it("reappears when the cursor position changes", () => {
    vi.useFakeTimers();
    const harness = renderCursorLabel(makePeer(), 3_000);

    act(() => {
      vi.advanceTimersByTime(3_001);
    });
    expect(harness.queryLabel()).toBeNull();

    harness.render(makePeer({ cursorPosition: { x: 168, y: 62 } }));
    const label = harness.queryLabel();
    expect(label).not.toBeNull();
    expect(label?.style.left).toBe("168px");
    expect(label?.style.top).toBe("62px");

    harness.unmount();
  });
});
