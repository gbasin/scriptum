// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useToastStore } from "../store/toast";
import { ToastViewport } from "./ToastViewport";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

beforeEach(() => {
  useToastStore.getState().reset();
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("ToastViewport", () => {
  it("renders toast messages and dismisses via button", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(<ToastViewport />);
    });
    act(() => {
      useToastStore.getState().pushToast("Copied link", {
        variant: "success",
      });
    });

    const toast = container.querySelector('[data-testid="toast-item"]');
    expect(toast?.textContent).toContain("Copied link");

    const toastId = useToastStore.getState().toasts[0]?.id;
    const dismissButton = container.querySelector(
      `[data-testid="toast-dismiss-${toastId}"]`,
    ) as HTMLButtonElement | null;
    act(() => {
      dismissButton?.click();
    });

    expect(useToastStore.getState().toasts).toHaveLength(0);

    act(() => {
      root.unmount();
    });
  });

  it("auto-dismisses toasts after duration", () => {
    vi.useFakeTimers();
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(<ToastViewport />);
      useToastStore.getState().pushToast("Saved", {
        durationMs: 600,
      });
    });
    expect(useToastStore.getState().toasts).toHaveLength(1);

    act(() => {
      vi.advanceTimersByTime(600);
    });

    expect(useToastStore.getState().toasts).toHaveLength(0);

    act(() => {
      root.unmount();
    });
    vi.useRealTimers();
  });
});
