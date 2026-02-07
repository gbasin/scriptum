import clsx from "clsx";
import styles from "./TabBar.module.css";

export interface OpenDocumentTab {
  id: string;
  path: string;
  title: string;
}

export interface TabBarProps {
  activeDocumentId: string | null;
  onCloseTab?: (documentId: string) => void;
  onSelectTab?: (documentId: string) => void;
  tabs: OpenDocumentTab[];
}

export function TabBar({
  activeDocumentId,
  onCloseTab,
  onSelectTab,
  tabs,
}: TabBarProps) {
  return (
    <nav
      aria-label="Open document tabs"
      className={styles.root}
      data-testid="tab-bar"
    >
      {tabs.length === 0 ? (
        <div className={styles.emptyState} data-testid="tab-bar-empty">
          No open documents
        </div>
      ) : (
        tabs.map((tab) => {
          const isActive = tab.id === activeDocumentId;
          return (
            <div
              className={clsx(styles.tab, isActive && styles.tabActive)}
              data-active={isActive || undefined}
              data-testid={`tab-${tab.id}`}
              key={tab.id}
            >
              <button
                className={clsx(
                  styles.selectTabButton,
                  isActive && styles.selectTabButtonActive,
                )}
                onClick={() => onSelectTab?.(tab.id)}
                title={tab.path}
                type="button"
              >
                {tab.title}
              </button>
              <button
                aria-label={`Close ${tab.title}`}
                className={styles.closeTabButton}
                data-testid={`tab-close-${tab.id}`}
                onClick={() => onCloseTab?.(tab.id)}
                type="button"
              >
                x
              </button>
            </div>
          );
        })
      )}
    </nav>
  );
}
