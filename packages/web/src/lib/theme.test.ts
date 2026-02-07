// @vitest-environment jsdom

import type { WorkspaceDensity, WorkspaceTheme } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import {
  applyAppearanceSettings,
  configureDaemonGitSyncPolling,
  GIT_SYNC_POLLING_EVENT,
  applyResolvedTheme,
  resolveThemePreference,
  startAppearanceSync,
  startGitSyncPollingSync,
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
  let matches = initialMatches;
  const mediaQuery = {
    get matches() {
      return matches;
    },
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
    emit: (nextMatches: boolean) => {
      matches = nextMatches;
      const event = {
        matches: nextMatches,
        media: mediaQuery.media,
      } as MediaQueryListEvent;
      for (const listener of listeners) {
        listener(event);
      }
    },
  } as FakeMediaQueryList;

  return mediaQuery;
}

function createThemeStore(
  initialTheme: WorkspaceTheme | undefined,
  initialDensity: WorkspaceDensity | undefined = "comfortable",
  initialFontSize = 15,
  initialGitSyncIntervalSeconds = 30,
) {
  let state: {
    activeWorkspace: {
      config: {
        appearance: {
          theme?: WorkspaceTheme;
          density?: WorkspaceDensity;
          fontSize?: number;
        };
        gitSync: {
          autoCommitIntervalSeconds?: number;
        };
      };
    };
  } = {
    activeWorkspace: {
      config: {
        appearance: {
          theme: initialTheme,
          density: initialDensity,
          fontSize: initialFontSize,
        },
        gitSync: {
          autoCommitIntervalSeconds: initialGitSyncIntervalSeconds,
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
            density: previous.activeWorkspace.config.appearance.density,
            fontSize: previous.activeWorkspace.config.appearance.fontSize,
          },
          gitSync: {
            autoCommitIntervalSeconds:
              previous.activeWorkspace.config.gitSync
                .autoCommitIntervalSeconds,
          },
        },
      },
    };
    for (const listener of listeners) {
      listener(state, previous);
    }
  };

  const setAppearance = (
    density: WorkspaceDensity | undefined,
    fontSize: number | undefined,
  ) => {
    const previous = state;
    state = {
      activeWorkspace: {
        config: {
          appearance: {
            theme: previous.activeWorkspace.config.appearance.theme,
            density,
            fontSize,
          },
          gitSync: {
            autoCommitIntervalSeconds:
              previous.activeWorkspace.config.gitSync
                .autoCommitIntervalSeconds,
          },
        },
      },
    };
    for (const listener of listeners) {
      listener(state, previous);
    }
  };

  const setGitSyncInterval = (seconds: number | undefined) => {
    const previous = state;
    state = {
      activeWorkspace: {
        config: {
          appearance: {
            theme: previous.activeWorkspace.config.appearance.theme,
            density: previous.activeWorkspace.config.appearance.density,
            fontSize: previous.activeWorkspace.config.appearance.fontSize,
          },
          gitSync: {
            autoCommitIntervalSeconds: seconds,
          },
        },
      },
    };
    for (const listener of listeners) {
      listener(state, previous);
    }
  };

  return { setAppearance, setGitSyncInterval, setTheme, store };
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

describe("applyAppearanceSettings", () => {
  it("applies density and base font-size css variable", () => {
    const root = document.createElement("html");
    applyAppearanceSettings(root, { density: "spacious", fontSizePx: 18 });
    expect(root.getAttribute("data-density")).toBe("spacious");
    expect(root.style.getPropertyValue("--font-size-base")).toBe("18px");
  });
});

describe("startAppearanceSync", () => {
  it("applies and updates appearance settings from store", () => {
    const root = document.createElement("html");
    const { setAppearance, store } = createThemeStore(
      "system",
      "comfortable",
      15,
    );
    const stop = startAppearanceSync(store, { root });

    expect(root.getAttribute("data-density")).toBe("comfortable");
    expect(root.style.getPropertyValue("--font-size-base")).toBe("15px");

    setAppearance("compact", 13);
    expect(root.getAttribute("data-density")).toBe("compact");
    expect(root.style.getPropertyValue("--font-size-base")).toBe("13px");

    setAppearance(undefined, undefined);
    expect(root.getAttribute("data-density")).toBe("comfortable");
    expect(root.style.getPropertyValue("--font-size-base")).toBe("15px");

    stop();
  });
});

describe("configureDaemonGitSyncPolling", () => {
  it("invokes daemon bridge and emits polling event", () => {
    const calls: number[] = [];
    const target = window as Window & typeof globalThis;
    const listeners: number[] = [];
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{ intervalSeconds: number }>).detail;
      listeners.push(detail.intervalSeconds);
    };
    target.addEventListener(GIT_SYNC_POLLING_EVENT, handler);

    (
      target as unknown as {
        __SCRIPTUM_DAEMON__?: { setGitSyncPollIntervalSeconds?: (n: number) => void };
      }
    ).__SCRIPTUM_DAEMON__ = {
      setGitSyncPollIntervalSeconds: (seconds) => {
        calls.push(seconds);
      },
    };

    configureDaemonGitSyncPolling(42, { target });
    expect(calls).toEqual([42]);
    expect(listeners).toEqual([42]);

    target.removeEventListener(GIT_SYNC_POLLING_EVENT, handler);
    delete (
      target as unknown as {
        __SCRIPTUM_DAEMON__?: { setGitSyncPollIntervalSeconds?: (n: number) => void };
      }
    ).__SCRIPTUM_DAEMON__;
  });
});

describe("startGitSyncPollingSync", () => {
  it("syncs workspace interval changes to daemon polling", () => {
    const calls: number[] = [];
    const target = window as Window & typeof globalThis;
    (
      target as unknown as {
        __SCRIPTUM_DAEMON__?: { setGitSyncPollIntervalSeconds?: (n: number) => void };
      }
    ).__SCRIPTUM_DAEMON__ = {
      setGitSyncPollIntervalSeconds: (seconds) => {
        calls.push(seconds);
      },
    };
    const { setGitSyncInterval, store } = createThemeStore(
      "system",
      "comfortable",
      15,
      30,
    );
    const stop = startGitSyncPollingSync(store, { target });
    expect(calls).toEqual([30]);

    setGitSyncInterval(45);
    expect(calls).toEqual([30, 45]);

    setGitSyncInterval(45);
    expect(calls).toEqual([30, 45]);

    stop();
    delete (
      target as unknown as {
        __SCRIPTUM_DAEMON__?: { setGitSyncPollIntervalSeconds?: (n: number) => void };
      }
    ).__SCRIPTUM_DAEMON__;
  });
});
