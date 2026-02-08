import { Component, type ErrorInfo, type ReactNode } from "react";
import styles from "./ErrorBoundary.module.css";

interface ErrorBoundaryProps {
  children: ReactNode;
  onReload?: () => void;
  title?: string;
  message?: string;
  reloadLabel?: string;
  testId?: string;
  inline?: boolean;
}

interface ErrorBoundaryState {
  hasError: boolean;
}

function defaultReload(): void {
  if (typeof window === "undefined") {
    return;
  }
  window.location.reload();
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  override state: ErrorBoundaryState = { hasError: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true };
  }

  override componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    console.error("Unhandled application error", error, errorInfo);
  }

  private readonly handleReload = () => {
    const onReload = this.props.onReload ?? defaultReload;
    onReload();
  };

  override render() {
    if (this.state.hasError) {
      const title = this.props.title ?? "Something went wrong";
      const message =
        this.props.message ??
        "Scriptum hit an unexpected error. Reload the app to recover.";
      const reloadLabel = this.props.reloadLabel ?? "Reload";
      const testId = this.props.testId ?? "app-error-boundary";

      return (
        <section
          aria-live="assertive"
          className={
            this.props.inline
              ? `${styles.fallback} ${styles.inlineFallback}`
              : styles.fallback
          }
          data-testid={testId}
        >
          <h1 className={styles.title}>{title}</h1>
          <p className={styles.message}>{message}</p>
          <button
            className={styles.reloadButton}
            data-testid={`${testId}-reload`}
            onClick={this.handleReload}
            type="button"
          >
            {reloadLabel}
          </button>
        </section>
      );
    }

    return this.props.children;
  }
}
