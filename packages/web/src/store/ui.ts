import { create, type StoreApi, type UseBoundStore } from "zustand";

export type SidebarPanel = "files" | "search" | "tags";
export type RightPanelTab = "outline" | "backlinks" | "comments";
const ONBOARDING_COMPLETED_STORAGE_KEY = "scriptum:onboarding:completed";

interface UiSnapshot {
  sidebarOpen: boolean;
  sidebarPanel: SidebarPanel;
  rightPanelOpen: boolean;
  rightPanelTab: RightPanelTab;
  commandPaletteOpen: boolean;
  activeModal: string | null;
  onboardingCompleted: boolean;
}

export interface UiStoreState extends UiSnapshot {
  toggleSidebar: () => void;
  setSidebarPanel: (panel: SidebarPanel) => void;
  toggleRightPanel: () => void;
  setRightPanelTab: (tab: RightPanelTab) => void;
  openCommandPalette: () => void;
  closeCommandPalette: () => void;
  openModal: (id: string) => void;
  closeModal: () => void;
  completeOnboarding: () => void;
  resetOnboarding: () => void;
  reset: () => void;
}

export type UiStore = UseBoundStore<StoreApi<UiStoreState>>;

const INITIAL_SNAPSHOT: UiSnapshot = {
  sidebarOpen: true,
  sidebarPanel: "files",
  rightPanelOpen: true,
  rightPanelTab: "outline",
  commandPaletteOpen: false,
  activeModal: null,
  onboardingCompleted: false,
};

function resolveOnboardingCompletedFromStorage(): boolean {
  try {
    if (typeof globalThis.localStorage === "undefined") {
      return false;
    }

    const candidate = globalThis.localStorage as Partial<Storage>;
    if (
      typeof candidate.getItem !== "function" ||
      typeof candidate.setItem !== "function" ||
      typeof candidate.removeItem !== "function"
    ) {
      return false;
    }

    return candidate.getItem(ONBOARDING_COMPLETED_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

function persistOnboardingCompleted(completed: boolean): void {
  try {
    if (typeof globalThis.localStorage === "undefined") {
      return;
    }

    const candidate = globalThis.localStorage as Partial<Storage>;
    if (
      typeof candidate.getItem !== "function" ||
      typeof candidate.setItem !== "function" ||
      typeof candidate.removeItem !== "function"
    ) {
      return;
    }

    if (completed) {
      candidate.setItem(ONBOARDING_COMPLETED_STORAGE_KEY, "true");
      return;
    }

    candidate.removeItem(ONBOARDING_COMPLETED_STORAGE_KEY);
  } catch {
    // Ignore localStorage write failures.
  }
}

export function createUiStore(initial: Partial<UiSnapshot> = {}): UiStore {
  const initialState: UiSnapshot = {
    ...INITIAL_SNAPSHOT,
    ...initial,
    onboardingCompleted:
      initial.onboardingCompleted ?? resolveOnboardingCompletedFromStorage(),
  };

  return create<UiStoreState>()((set, get) => ({
    ...initialState,

    toggleSidebar: () => {
      set({ sidebarOpen: !get().sidebarOpen });
    },

    setSidebarPanel: (panel) => {
      set({
        sidebarPanel: panel,
        sidebarOpen: true,
      });
    },

    toggleRightPanel: () => {
      set({ rightPanelOpen: !get().rightPanelOpen });
    },

    setRightPanelTab: (tab) => {
      set({
        rightPanelTab: tab,
        rightPanelOpen: true,
      });
    },

    openCommandPalette: () => {
      set({ commandPaletteOpen: true });
    },

    closeCommandPalette: () => {
      set({ commandPaletteOpen: false });
    },

    openModal: (id) => {
      set({ activeModal: id });
    },

    closeModal: () => {
      set({ activeModal: null });
    },

    completeOnboarding: () => {
      persistOnboardingCompleted(true);
      set({ onboardingCompleted: true });
    },

    resetOnboarding: () => {
      persistOnboardingCompleted(false);
      set({ onboardingCompleted: false });
    },

    reset: () => {
      set({
        ...INITIAL_SNAPSHOT,
        onboardingCompleted: get().onboardingCompleted,
      });
    },
  }));
}

export const useUiStore = createUiStore();
