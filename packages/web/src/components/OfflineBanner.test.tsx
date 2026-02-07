// @vitest-environment jsdom

import type { ComponentProps } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OfflineBanner } from "./OfflineBanner";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function renderOfflineBanner(props: ComponentProps<typeof OfflineBanner>) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  const render = (nextProps: ComponentProps<typeof OfflineBanner>) => {
    act(() => {
      root.render(<OfflineBanner {...nextProps} />);
    });
  };

  render(props);

  return {
    container,
    rerender: (nextProps: ComponentProps<typeof OfflineBanner>) =>
      render(nextProps),
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

describe("OfflineBanner", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.useRealTimers();
  });

  it("shows offline warning when disconnected", () => {
    const harness = renderOfflineBanner({
      status: "offline",
      reconnectProgress: null,
    });

    expect(harness.container.textContent).toContain(
      "You are offline — changes will sync when reconnected.",
    );
    expect(
      harness.container.querySelector('[data-testid="offline-banner-dismiss"]'),
    ).not.toBeNull();

    harness.unmount();
  });

  it("shows reconnect progress while syncing backlog", () => {
    const harness = renderOfflineBanner({
      status: "reconnecting",
      reconnectProgress: { syncedUpdates: 847, totalUpdates: 1203 },
    });

    expect(harness.container.textContent).toContain(
      "Syncing... 847/1,203 updates",
    );
    expect(
      harness.container.querySelector('[data-testid="offline-banner-dismiss"]'),
    ).toBeNull();

    harness.unmount();
  });

  it("hides content while synced", () => {
    const harness = renderOfflineBanner({
      status: "synced",
      reconnectProgress: null,
    });

    expect(
      harness.container.querySelector('[data-testid="offline-banner-message"]'),
    ).toBeNull();

    harness.unmount();
  });

  it("reappears after dismissal timeout when still offline", () => {
    vi.useFakeTimers();
    const harness = renderOfflineBanner({
      status: "offline",
      reconnectProgress: null,
      reappearAfterMs: 30_000,
    });

    const dismissButton = harness.container.querySelector(
      '[data-testid="offline-banner-dismiss"]',
    ) as HTMLButtonElement | null;
    expect(dismissButton).not.toBeNull();

    act(() => {
      dismissButton?.click();
    });

    expect(
      harness.container.querySelector('[data-testid="offline-banner-message"]'),
    ).toBeNull();

    act(() => {
      vi.advanceTimersByTime(30_000);
    });

    expect(harness.container.textContent).toContain(
      "You are offline — changes will sync when reconnected.",
    );

    harness.unmount();
  });
});
