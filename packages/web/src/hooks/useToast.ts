import { useMemo } from "react";
import { type ToastVariant, useToastStore } from "../store/toast";

export interface UseToastApi {
  show: (
    message: string,
    options?: { durationMs?: number; variant?: ToastVariant },
  ) => string;
  success: (message: string, options?: { durationMs?: number }) => string;
  error: (message: string, options?: { durationMs?: number }) => string;
  info: (message: string, options?: { durationMs?: number }) => string;
}

export function useToast(): UseToastApi {
  const pushToast = useToastStore((state) => state.pushToast);

  return useMemo(
    () => ({
      show: (message, options) => pushToast(message, options),
      success: (message, options) =>
        pushToast(message, { ...options, variant: "success" }),
      error: (message, options) =>
        pushToast(message, { ...options, variant: "error" }),
      info: (message, options) =>
        pushToast(message, { ...options, variant: "info" }),
    }),
    [pushToast],
  );
}
