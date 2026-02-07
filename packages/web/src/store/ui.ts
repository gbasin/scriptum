import { create, type StoreApi, type UseBoundStore } from "zustand";

export type SidebarPanel = "files" | "search" | "tags";
export type RightPanelTab = "outline" | "backlinks" | "comments";

interface UiSnapshot {
  sidebarOpen: boolean;
  sidebarPanel: SidebarPanel;
  rightPanelOpen: boolean;
  rightPanelTab: RightPanelTab;
  commandPaletteOpen: boolean;
  activeModal: string | null;
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
};

export function createUiStore(initial: Partial<UiSnapshot> = {}): UiStore {
  const initialState: UiSnapshot = { ...INITIAL_SNAPSHOT, ...initial };

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

    reset: () => {
      set({ ...INITIAL_SNAPSHOT });
    },
  }));
}

export const useUiStore = createUiStore();
