import { create, type StoreApi, type UseBoundStore } from "zustand";

export type ToastVariant = "success" | "error" | "info";

export interface Toast {
  id: string;
  message: string;
  variant: ToastVariant;
  durationMs: number;
}

interface ToastSnapshot {
  toasts: Toast[];
}

export interface ToastStoreState extends ToastSnapshot {
  pushToast: (
    message: string,
    options?: Partial<Pick<Toast, "durationMs" | "variant">>,
  ) => string;
  dismissToast: (toastId: string) => void;
  clearToasts: () => void;
  reset: () => void;
}

export type ToastStore = UseBoundStore<StoreApi<ToastStoreState>>;

const DEFAULT_DURATION_MS = 3200;
const DEFAULT_VARIANT: ToastVariant = "info";
const INITIAL_SNAPSHOT: ToastSnapshot = { toasts: [] };

function createToastId(): string {
  const token = Math.floor(Math.random() * 1e9)
    .toString(36)
    .padStart(6, "0");
  return `toast-${Date.now().toString(36)}-${token}`;
}

function normalizeDurationMs(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return DEFAULT_DURATION_MS;
  }
  const rounded = Math.floor(value);
  return rounded >= 500 ? rounded : 500;
}

function normalizeVariant(value: unknown): ToastVariant {
  return value === "success" || value === "error" || value === "info"
    ? value
    : DEFAULT_VARIANT;
}

export function createToastStore(initial: Partial<ToastSnapshot> = {}): ToastStore {
  const initialState: ToastSnapshot = { ...INITIAL_SNAPSHOT, ...initial };

  return create<ToastStoreState>()((set) => ({
    ...initialState,
    pushToast: (message, options) => {
      const toast: Toast = {
        id: createToastId(),
        message,
        variant: normalizeVariant(options?.variant),
        durationMs: normalizeDurationMs(options?.durationMs),
      };
      set((state) => ({ toasts: [...state.toasts, toast] }));
      return toast.id;
    },
    dismissToast: (toastId) => {
      set((state) => ({
        toasts: state.toasts.filter((toast) => toast.id !== toastId),
      }));
    },
    clearToasts: () => {
      set({ toasts: [] });
    },
    reset: () => {
      set({ ...INITIAL_SNAPSHOT });
    },
  }));
}

export const useToastStore = createToastStore();
