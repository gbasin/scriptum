import type { WorkspaceDensity, WorkspaceTheme } from "@scriptum/shared";

const DEFAULT_DENSITY: WorkspaceDensity = "comfortable";
const DEFAULT_FONT_SIZE_PX = 15;
const MIN_FONT_SIZE_PX = 10;
const MAX_FONT_SIZE_PX = 32;
const DEFAULT_GIT_SYNC_INTERVAL_SECONDS = 30;
const MIN_GIT_SYNC_INTERVAL_SECONDS = 5;

export const GIT_SYNC_POLLING_EVENT = "scriptum:git-sync-polling-updated";

export type ResolvedTheme = "dark" | "light";
export type ThemePreference = WorkspaceTheme | "system";

export interface ThemeStoreState {
  activeWorkspace?: {
    config?: {
      appearance?: {
        theme?: WorkspaceTheme;
        density?: WorkspaceDensity;
        fontSize?: number;
        editorFontSizePx?: number;
      };
      gitSync?: {
        autoCommitIntervalSeconds?: number;
      };
    };
  } | null;
}

export interface ThemeStore {
  getState: () => ThemeStoreState;
  subscribe: (
    listener: (state: ThemeStoreState, previousState: ThemeStoreState) => void,
  ) => () => void;
}

export interface ThemeSyncOptions {
  matchMedia?: ((query: string) => MediaQueryList) | null;
  root?: HTMLElement;
}

export interface AppearanceSyncOptions {
  root?: HTMLElement;
}

export interface GitSyncPollingOptions {
  target?: Window & typeof globalThis;
}

export interface DaemonBridge {
  setGitSyncPollIntervalSeconds?: (seconds: number) => void;
}

export interface ResolvedAppearance {
  density: WorkspaceDensity;
  fontSizePx: number;
}

export function resolveThemePreference(
  preference: WorkspaceTheme | undefined,
  prefersDark: boolean,
): ResolvedTheme {
  if (preference === "dark") {
    return "dark";
  }
  if (preference === "light") {
    return "light";
  }
  return prefersDark ? "dark" : "light";
}

export function applyResolvedTheme(
  root: HTMLElement,
  resolvedTheme: ResolvedTheme,
): void {
  root.classList.remove("dark", "light");
  root.classList.add(resolvedTheme);
  root.setAttribute("data-theme", resolvedTheme);
}

export function applyAppearanceSettings(
  root: HTMLElement,
  appearance: ResolvedAppearance,
): void {
  root.setAttribute("data-density", appearance.density);
  root.style.setProperty("--font-size-base", `${appearance.fontSizePx}px`);
}

function currentThemePreference(state: ThemeStoreState): ThemePreference {
  return state.activeWorkspace?.config?.appearance?.theme ?? "system";
}

function resolveDensity(value: unknown): WorkspaceDensity {
  return value === "compact" || value === "comfortable" || value === "spacious"
    ? value
    : DEFAULT_DENSITY;
}

function clampFontSizePx(value: number): number {
  const rounded = Math.floor(value);
  if (rounded < MIN_FONT_SIZE_PX) {
    return MIN_FONT_SIZE_PX;
  }
  if (rounded > MAX_FONT_SIZE_PX) {
    return MAX_FONT_SIZE_PX;
  }
  return rounded;
}

function resolveFontSizePx(
  appearance: ThemeStoreState["activeWorkspace"],
): number {
  const configured = appearance?.config?.appearance;
  const value = configured?.fontSize ?? configured?.editorFontSizePx;
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return DEFAULT_FONT_SIZE_PX;
  }
  return clampFontSizePx(value);
}

function currentAppearance(state: ThemeStoreState): ResolvedAppearance {
  return {
    density: resolveDensity(state.activeWorkspace?.config?.appearance?.density),
    fontSizePx: resolveFontSizePx(state.activeWorkspace),
  };
}

type LegacyMediaQueryList = MediaQueryList & {
  addListener?: (listener: (event: MediaQueryListEvent) => void) => void;
  removeListener?: (listener: (event: MediaQueryListEvent) => void) => void;
};

function subscribeToMediaQuery(
  mediaQuery: MediaQueryList,
  listener: (event: MediaQueryListEvent) => void,
): () => void {
  const query = mediaQuery as LegacyMediaQueryList;
  if (typeof query.addEventListener === "function") {
    query.addEventListener("change", listener);
    return () => {
      query.removeEventListener("change", listener);
    };
  }

  if (
    typeof query.addListener === "function" &&
    typeof query.removeListener === "function"
  ) {
    query.addListener(listener);
    return () => {
      query.removeListener?.(listener);
    };
  }

  return () => {};
}

export function startThemeSync(
  store: ThemeStore,
  options: ThemeSyncOptions = {},
): () => void {
  if (typeof document === "undefined") {
    return () => {};
  }

  const root = options.root ?? document.documentElement;
  const matchMediaFn =
    options.matchMedia ??
    (typeof window !== "undefined" && "matchMedia" in window
      ? window.matchMedia.bind(window)
      : null);
  const mediaQuery = matchMediaFn
    ? matchMediaFn("(prefers-color-scheme: dark)")
    : null;

  let themePreference = currentThemePreference(store.getState());
  let detachMediaListener: () => void = () => {};

  const applyTheme = (prefersDark: boolean) => {
    applyResolvedTheme(
      root,
      resolveThemePreference(themePreference, prefersDark),
    );
  };

  const refreshMediaListener = () => {
    detachMediaListener();
    detachMediaListener = () => {};

    if (themePreference !== "system" || !mediaQuery) {
      return;
    }

    detachMediaListener = subscribeToMediaQuery(mediaQuery, (event) => {
      applyTheme(event.matches);
    });
  };

  applyTheme(mediaQuery?.matches ?? false);
  refreshMediaListener();

  const unsubscribeStore = store.subscribe((state) => {
    const nextThemePreference = currentThemePreference(state);
    if (nextThemePreference === themePreference) {
      return;
    }
    themePreference = nextThemePreference;
    applyTheme(mediaQuery?.matches ?? false);
    refreshMediaListener();
  });

  return () => {
    unsubscribeStore();
    detachMediaListener();
  };
}

export function startAppearanceSync(
  store: ThemeStore,
  options: AppearanceSyncOptions = {},
): () => void {
  if (typeof document === "undefined") {
    return () => {};
  }

  const root = options.root ?? document.documentElement;
  let appearance = currentAppearance(store.getState());
  applyAppearanceSettings(root, appearance);

  const unsubscribeStore = store.subscribe((state) => {
    const nextAppearance = currentAppearance(state);
    if (
      nextAppearance.density === appearance.density &&
      nextAppearance.fontSizePx === appearance.fontSizePx
    ) {
      return;
    }
    appearance = nextAppearance;
    applyAppearanceSettings(root, appearance);
  });

  return () => {
    unsubscribeStore();
  };
}

function resolveGitSyncPollingIntervalSeconds(state: ThemeStoreState): number {
  const raw = state.activeWorkspace?.config?.gitSync?.autoCommitIntervalSeconds;
  if (typeof raw !== "number" || !Number.isFinite(raw)) {
    return DEFAULT_GIT_SYNC_INTERVAL_SECONDS;
  }
  return Math.max(MIN_GIT_SYNC_INTERVAL_SECONDS, Math.floor(raw));
}

export function configureDaemonGitSyncPolling(
  intervalSeconds: number,
  options: GitSyncPollingOptions = {},
): void {
  const target =
    options.target ?? (typeof window === "undefined" ? undefined : window);
  if (!target) {
    return;
  }

  const bridge = target as unknown as { __SCRIPTUM_DAEMON__?: DaemonBridge };
  bridge.__SCRIPTUM_DAEMON__?.setGitSyncPollIntervalSeconds?.(intervalSeconds);
  target.dispatchEvent(
    new CustomEvent(GIT_SYNC_POLLING_EVENT, {
      detail: { intervalSeconds },
    }),
  );
}

export function startGitSyncPollingSync(
  store: ThemeStore,
  options: GitSyncPollingOptions = {},
): () => void {
  let intervalSeconds = resolveGitSyncPollingIntervalSeconds(store.getState());
  configureDaemonGitSyncPolling(intervalSeconds, options);

  const unsubscribeStore = store.subscribe((state) => {
    const nextIntervalSeconds = resolveGitSyncPollingIntervalSeconds(state);
    if (nextIntervalSeconds === intervalSeconds) {
      return;
    }
    intervalSeconds = nextIntervalSeconds;
    configureDaemonGitSyncPolling(intervalSeconds, options);
  });

  return () => {
    unsubscribeStore();
  };
}
