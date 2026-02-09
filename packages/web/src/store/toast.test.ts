import { describe, expect, it } from "vitest";
import { createToastStore } from "./toast";

describe("toast store", () => {
  it("pushes and dismisses toasts", () => {
    const store = createToastStore();

    const toastId = store.getState().pushToast("Saved");
    expect(store.getState().toasts).toHaveLength(1);
    expect(store.getState().toasts[0]?.id).toBe(toastId);
    expect(store.getState().toasts[0]?.variant).toBe("info");

    store.getState().dismissToast(toastId);
    expect(store.getState().toasts).toHaveLength(0);
  });

  it("normalizes toast options", () => {
    const store = createToastStore();

    store.getState().pushToast("Copied link", {
      durationMs: 100,
      variant: "success",
    });
    const toast = store.getState().toasts[0];
    expect(toast?.variant).toBe("success");
    expect(toast?.durationMs).toBe(500);
  });

  it("clears and resets state", () => {
    const store = createToastStore();
    store.getState().pushToast("One");
    store.getState().pushToast("Two");
    expect(store.getState().toasts).toHaveLength(2);

    store.getState().clearToasts();
    expect(store.getState().toasts).toHaveLength(0);

    store.getState().pushToast("Three");
    expect(store.getState().toasts).toHaveLength(1);
    store.getState().reset();
    expect(store.getState().toasts).toHaveLength(0);
  });

  it("keeps only the newest five toasts", () => {
    const store = createToastStore();

    for (let index = 1; index <= 7; index += 1) {
      store.getState().pushToast(`Toast ${index}`);
    }

    expect(store.getState().toasts).toHaveLength(5);
    expect(store.getState().toasts.map((toast) => toast.message)).toEqual([
      "Toast 3",
      "Toast 4",
      "Toast 5",
      "Toast 6",
      "Toast 7",
    ]);
  });
});
