import type { WorkspaceTheme } from "@scriptum/shared";

export type ResolvedTheme = "dark" | "light";
export type ThemePreference = WorkspaceTheme | "system";

export interface ThemeStoreState {
  activeWorkspace?: {
    config?: {
      appearance?: {
        theme?: WorkspaceTheme;
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

function currentThemePreference(state: ThemeStoreState): ThemePreference {
  return state.activeWorkspace?.config?.appearance?.theme ?? "system";
}

function subscribeToMediaQuery(
  mediaQuery: MediaQueryList,
  listener: (event: MediaQueryListEvent) => void,
): () => void {
  if ("addEventListener" in mediaQuery) {
    mediaQuery.addEventListener("change", listener);
    return () => {
      mediaQuery.removeEventListener("change", listener);
    };
  }

  mediaQuery.addListener(listener);
  return () => {
    mediaQuery.removeListener(listener);
  };
}

export function startThemeSync(
  store: ThemeStore,
  options: ThemeSyncOptions = {},
): () => void {
  if (typeof document === "undefined") {
    return () => undefined;
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
  let detachMediaListener = () => undefined;

  const applyTheme = (prefersDark: boolean) => {
    applyResolvedTheme(root, resolveThemePreference(themePreference, prefersDark));
  };

  const refreshMediaListener = () => {
    detachMediaListener();
    detachMediaListener = () => undefined;

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
