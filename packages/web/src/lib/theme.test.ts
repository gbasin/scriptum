// @vitest-environment jsdom

import type { WorkspaceTheme } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import {
  applyResolvedTheme,
  resolveThemePreference,
  startThemeSync,
  type ThemeStore,
} from "./theme";

describe("resolveThemePreference", () => {
  it("maps explicit theme choices directly", () => {
    expect(resolveThemePreference("light", true)).toBe("light");
    expect(resolveThemePreference("dark", false)).toBe("dark");
  });

  it("uses prefers-color-scheme when theme is system or undefined", () => {
    expect(resolveThemePreference("system", true)).toBe("dark");
    expect(resolveThemePreference("system", false)).toBe("light");
    expect(resolveThemePreference(undefined, true)).toBe("dark");
    expect(resolveThemePreference(undefined, false)).toBe("light");
  });
});

describe("applyResolvedTheme", () => {
  it("applies class and data-theme attribute on html element", () => {
    const root = document.createElement("html");
    root.classList.add("light");

    applyResolvedTheme(root, "dark");
    expect(root.classList.contains("dark")).toBe(true);
    expect(root.classList.contains("light")).toBe(false);
    expect(root.getAttribute("data-theme")).toBe("dark");

    applyResolvedTheme(root, "light");
    expect(root.classList.contains("dark")).toBe(false);
    expect(root.classList.contains("light")).toBe(true);
    expect(root.getAttribute("data-theme")).toBe("light");
  });
});

interface FakeMediaQueryList extends MediaQueryList {
  emit: (matches: boolean) => void;
}

function createFakeMediaQueryList(initialMatches: boolean): FakeMediaQueryList {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mediaQuery = {
    matches: initialMatches,
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: (
      _type: "change",
      listener: (event: MediaQueryListEvent) => void,
    ) => {
      listeners.add(listener);
    },
    removeEventListener: (
      _type: "change",
      listener: (event: MediaQueryListEvent) => void,
    ) => {
      listeners.delete(listener);
    },
    addListener: (listener: (event: MediaQueryListEvent) => void) => {
      listeners.add(listener);
    },
    removeListener: (listener: (event: MediaQueryListEvent) => void) => {
      listeners.delete(listener);
    },
    dispatchEvent: () => true,
    emit: (matches: boolean) => {
      mediaQuery.matches = matches;
      const event = { matches, media: mediaQuery.media } as MediaQueryListEvent;
      for (const listener of listeners) {
        listener(event);
      }
    },
  } as FakeMediaQueryList;

  return mediaQuery;
}

function createThemeStore(initialTheme: WorkspaceTheme | undefined) {
  let state: {
    activeWorkspace: {
      config: {
        appearance: {
          theme?: WorkspaceTheme;
        };
      };
    };
  } = {
    activeWorkspace: {
      config: {
        appearance: {
          theme: initialTheme,
        },
      },
    },
  };
  const listeners = new Set<
    (nextState: typeof state, previousState: typeof state) => void
  >();

  const store: ThemeStore = {
    getState: () => state,
    subscribe: (listener) => {
      listeners.add(
        listener as (nextState: typeof state, previousState: typeof state) => void,
      );
      return () => {
        listeners.delete(
          listener as (
            nextState: typeof state,
            previousState: typeof state,
          ) => void,
        );
      };
    },
  };

  const setTheme = (theme: WorkspaceTheme | undefined) => {
    const previous = state;
    state = {
      activeWorkspace: {
        config: {
          appearance: {
            theme,
          },
        },
      },
    };
    for (const listener of listeners) {
      listener(state, previous);
    }
  };

  return { setTheme, store };
}

describe("startThemeSync", () => {
  it("applies and updates theme from store and system preference", () => {
    const root = document.createElement("html");
    const mediaQuery = createFakeMediaQueryList(false);
    const { setTheme, store } = createThemeStore("system");
    const stop = startThemeSync(store, {
      matchMedia: () => mediaQuery,
      root,
    });

    expect(root.getAttribute("data-theme")).toBe("light");

    mediaQuery.emit(true);
    expect(root.getAttribute("data-theme")).toBe("dark");

    setTheme("light");
    expect(root.getAttribute("data-theme")).toBe("light");

    mediaQuery.emit(false);
    expect(root.getAttribute("data-theme")).toBe("light");

    setTheme("dark");
    expect(root.getAttribute("data-theme")).toBe("dark");

    stop();
  });
});
