import { describe, expect, it } from "vitest";
import { createUiStore } from "./ui";

describe("ui store", () => {
  it("starts with expected defaults", () => {
    const store = createUiStore();

    expect(store.getState().sidebarOpen).toBe(true);
    expect(store.getState().sidebarPanel).toBe("files");
    expect(store.getState().rightPanelOpen).toBe(false);
    expect(store.getState().rightPanelTab).toBe("outline");
    expect(store.getState().commandPaletteOpen).toBe(false);
    expect(store.getState().activeModal).toBeNull();
  });

  it("toggles sidebar and sets sidebar panel", () => {
    const store = createUiStore();

    store.getState().toggleSidebar();
    expect(store.getState().sidebarOpen).toBe(false);

    store.getState().setSidebarPanel("search");
    expect(store.getState().sidebarPanel).toBe("search");
    expect(store.getState().sidebarOpen).toBe(true);
  });

  it("toggles right panel and sets tab", () => {
    const store = createUiStore();

    store.getState().toggleRightPanel();
    expect(store.getState().rightPanelOpen).toBe(true);

    store.getState().toggleRightPanel();
    expect(store.getState().rightPanelOpen).toBe(false);

    store.getState().setRightPanelTab("comments");
    expect(store.getState().rightPanelTab).toBe("comments");
    expect(store.getState().rightPanelOpen).toBe(true);
  });

  it("opens and closes command palette", () => {
    const store = createUiStore();

    store.getState().openCommandPalette();
    expect(store.getState().commandPaletteOpen).toBe(true);

    store.getState().closeCommandPalette();
    expect(store.getState().commandPaletteOpen).toBe(false);
  });

  it("opens and closes modal", () => {
    const store = createUiStore();

    store.getState().openModal("share-link");
    expect(store.getState().activeModal).toBe("share-link");

    store.getState().closeModal();
    expect(store.getState().activeModal).toBeNull();
  });

  it("resets all ui state to defaults", () => {
    const store = createUiStore({
      sidebarOpen: false,
      sidebarPanel: "tags",
      rightPanelOpen: true,
      rightPanelTab: "comments",
      commandPaletteOpen: true,
      activeModal: "rename-document",
    });

    store.getState().reset();

    expect(store.getState().sidebarOpen).toBe(true);
    expect(store.getState().sidebarPanel).toBe("files");
    expect(store.getState().rightPanelOpen).toBe(false);
    expect(store.getState().rightPanelTab).toBe("outline");
    expect(store.getState().commandPaletteOpen).toBe(false);
    expect(store.getState().activeModal).toBeNull();
  });
});
