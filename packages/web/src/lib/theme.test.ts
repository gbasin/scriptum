// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { WorkspaceDensity, WorkspaceTheme } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import {
  applyAppearanceSettings,
  applyResolvedTheme,
  configureDaemonGitSyncPolling,
  GIT_SYNC_POLLING_EVENT,
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

interface LinearRgb {
  r: number;
  g: number;
  b: number;
}

interface OklchColor {
  l: number;
  c: number;
  h: number;
}

const TOKENS_CSS = readFileSync(
  resolve(dirname(fileURLToPath(import.meta.url)), "../styles/tokens.css"),
  "utf8",
);

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function cssBlock(selector: string): string {
  const matcher = new RegExp(`${escapeRegExp(selector)}\\s*\\{([\\s\\S]*?)\\n\\}`);
  const match = TOKENS_CSS.match(matcher);
  if (!match) {
    throw new Error(`Missing CSS block: ${selector}`);
  }
  return match[1];
}

function tokenFromBlock(block: string, token: string): string {
  const matcher = new RegExp(`--${escapeRegExp(token)}:\\s*([^;]+);`);
  const match = block.match(matcher);
  if (!match) {
    throw new Error(`Missing token: ${token}`);
  }
  return match[1].trim();
}

function parseHexColor(value: string): LinearRgb {
  const normalized = value.trim().toLowerCase();
  if (!/^#[0-9a-f]{6}$/.test(normalized)) {
    throw new Error(`Expected #RRGGBB color, got ${value}`);
  }
  const toLinearChannel = (hex: string): number => {
    const srgb = Number.parseInt(hex, 16) / 255;
    return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  return {
    b: toLinearChannel(normalized.slice(5, 7)),
    g: toLinearChannel(normalized.slice(3, 5)),
    r: toLinearChannel(normalized.slice(1, 3)),
  };
}

function parseOklchColor(value: string): OklchColor {
  const match = value
    .trim()
    .match(/^oklch\(([\d.]+)\s+([\d.]+)\s+(-?[\d.]+)\)$/);
  if (!match) {
    throw new Error(`Expected oklch() color, got ${value}`);
  }
  return {
    c: Number.parseFloat(match[2]),
    h: Number.parseFloat(match[3]),
    l: Number.parseFloat(match[1]),
  };
}

function clamp(value: number): number {
  return Math.max(0, Math.min(1, value));
}

function oklchToLinearRgb(color: OklchColor): LinearRgb {
  const hueRadians = (color.h * Math.PI) / 180;
  const a = color.c * Math.cos(hueRadians);
  const b = color.c * Math.sin(hueRadians);

  const lPrime = color.l + 0.3963377774 * a + 0.2158037573 * b;
  const mPrime = color.l - 0.1055613458 * a - 0.0638541728 * b;
  const sPrime = color.l - 0.0894841775 * a - 1.291485548 * b;

  const l = lPrime ** 3;
  const m = mPrime ** 3;
  const s = sPrime ** 3;

  return {
    b: clamp(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    g: clamp(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    r: clamp(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
  };
}

function relativeLuminance(color: LinearRgb): number {
  return 0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b;
}

function contrastRatio(first: LinearRgb, second: LinearRgb): number {
  const firstLum = relativeLuminance(first);
  const secondLum = relativeLuminance(second);
  const lighter = Math.max(firstLum, secondLum);
  const darker = Math.min(firstLum, secondLum);
  return (lighter + 0.05) / (darker + 0.05);
}

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
        listener as (
          nextState: typeof state,
          previousState: typeof state,
        ) => void,
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
              previous.activeWorkspace.config.gitSync.autoCommitIntervalSeconds,
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
              previous.activeWorkspace.config.gitSync.autoCommitIntervalSeconds,
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
        __SCRIPTUM_DAEMON__?: {
          setGitSyncPollIntervalSeconds?: (n: number) => void;
        };
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
        __SCRIPTUM_DAEMON__?: {
          setGitSyncPollIntervalSeconds?: (n: number) => void;
        };
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
        __SCRIPTUM_DAEMON__?: {
          setGitSyncPollIntervalSeconds?: (n: number) => void;
        };
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
        __SCRIPTUM_DAEMON__?: {
          setGitSyncPollIntervalSeconds?: (n: number) => void;
        };
      }
    ).__SCRIPTUM_DAEMON__;
  });
});

describe("light theme accent tokens", () => {
  it("override accent tokens instead of inheriting dark defaults", () => {
    const rootBlock = cssBlock(":root");
    const lightBlock = cssBlock('[data-theme="light"]');

    expect(tokenFromBlock(lightBlock, "color-accent-hex")).not.toBe(
      tokenFromBlock(rootBlock, "color-accent-hex"),
    );
    expect(tokenFromBlock(lightBlock, "color-accent")).not.toBe(
      tokenFromBlock(rootBlock, "color-accent"),
    );
    expect(tokenFromBlock(lightBlock, "color-accent-soft")).not.toBe(
      tokenFromBlock(rootBlock, "color-accent-soft"),
    );
  });

  it("meet WCAG AA contrast against the light canvas", () => {
    const lightBlock = cssBlock('[data-theme="light"]');
    const canvas = oklchToLinearRgb(
      parseOklchColor(tokenFromBlock(lightBlock, "color-bg-canvas")),
    );
    const accentHex = parseHexColor(tokenFromBlock(lightBlock, "color-accent-hex"));
    const accentSoft = oklchToLinearRgb(
      parseOklchColor(tokenFromBlock(lightBlock, "color-accent-soft")),
    );

    expect(contrastRatio(accentHex, canvas)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(accentSoft, canvas)).toBeGreaterThanOrEqual(4.5);
  });
});
