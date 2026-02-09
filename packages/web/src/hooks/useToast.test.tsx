// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ToastViewport } from "../components/ToastViewport";
import { useToastStore } from "../store/toast";
import type { UseToastApi } from "./useToast";
import { useToast } from "./useToast";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

interface HookHarness {
  container: HTMLDivElement;
  getApi: () => UseToastApi;
  unmount: () => void;
}

function renderUseToastHarness(includeViewport = false): HookHarness {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  let api: UseToastApi | undefined;

  function HookComponent() {
    api = useToast();
    return includeViewport ? <ToastViewport /> : null;
  }

  act(() => {
    root.render(<HookComponent />);
  });

  return {
    container,
    getApi: () => {
      if (!api) {
        throw new Error("useToast API not initialized");
      }
      return api;
    },
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

beforeEach(() => {
  useToastStore.getState().reset();
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  vi.useRealTimers();
});

describe("useToast", () => {
  it("creates toasts with show/success/error/info helpers", () => {
    const harness = renderUseToastHarness();
    const toast = harness.getApi();

    act(() => {
      toast.show("Saved");
      toast.success("Published");
      toast.error("Failed");
      toast.info("Heads up", { durationMs: 100 });
    });

    expect(useToastStore.getState().toasts).toEqual([
      expect.objectContaining({ message: "Saved", variant: "info" }),
      expect.objectContaining({ message: "Published", variant: "success" }),
      expect.objectContaining({ message: "Failed", variant: "error" }),
      expect.objectContaining({
        durationMs: 500,
        message: "Heads up",
        variant: "info",
      }),
    ]);

    harness.unmount();
  });

  it("supports manual dismiss through the viewport action", () => {
    const harness = renderUseToastHarness(true);
    const toast = harness.getApi();

    let toastId = "";
    act(() => {
      toastId = toast.show("Dismiss me");
    });

    const dismissButton = harness.container.querySelector(
      `[data-testid="toast-dismiss-${toastId}"]`,
    ) as HTMLButtonElement | null;
    act(() => {
      dismissButton?.click();
    });

    expect(useToastStore.getState().toasts).toHaveLength(0);

    harness.unmount();
  });

  it("auto-dismisses toasts after the configured duration", () => {
    vi.useFakeTimers();
    const harness = renderUseToastHarness(true);
    const toast = harness.getApi();

    act(() => {
      toast.show("Soon gone", { durationMs: 650 });
    });
    expect(useToastStore.getState().toasts).toHaveLength(1);

    act(() => {
      vi.advanceTimersByTime(650);
    });

    expect(useToastStore.getState().toasts).toHaveLength(0);

    harness.unmount();
  });

  it("respects the toast stack limit by keeping only newest entries", () => {
    const harness = renderUseToastHarness();
    const toast = harness.getApi();

    act(() => {
      for (let index = 1; index <= 7; index += 1) {
        toast.show(`Toast ${index}`);
      }
    });

    expect(
      useToastStore.getState().toasts.map((entry) => entry.message),
    ).toEqual(["Toast 3", "Toast 4", "Toast 5", "Toast 6", "Toast 7"]);

    harness.unmount();
  });

  it("keeps repeated messages distinct with unique toast ids", () => {
    const harness = renderUseToastHarness();
    const toast = harness.getApi();

    let firstId = "";
    let secondId = "";
    act(() => {
      firstId = toast.show("Repeated");
      secondId = toast.show("Repeated");
    });

    expect(firstId).not.toBe(secondId);
    expect(useToastStore.getState().toasts).toHaveLength(2);

    harness.unmount();
  });
});
