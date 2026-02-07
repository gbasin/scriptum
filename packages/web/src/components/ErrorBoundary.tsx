import { Component, type ErrorInfo, type ReactNode } from "react";
import styles from "./ErrorBoundary.module.css";

interface ErrorBoundaryProps {
  children: ReactNode;
  onReload?: () => void;
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
      return (
        <section
          aria-live="assertive"
          className={styles.fallback}
          data-testid="app-error-boundary"
        >
          <h1 className={styles.title}>Something went wrong</h1>
          <p className={styles.message}>
            Scriptum hit an unexpected error. Reload the app to recover.
          </p>
          <button
            className={styles.reloadButton}
            data-testid="app-error-reload"
            onClick={this.handleReload}
            type="button"
          >
            Reload
          </button>
        </section>
      );
    }

    return this.props.children;
  }
}
