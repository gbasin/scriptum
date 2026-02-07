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
      data-testid="tab-bar"
      style={{
        alignItems: "stretch",
        borderBottom: "1px solid #d1d5db",
        display: "flex",
        gap: "0.25rem",
        overflowX: "auto",
        paddingBottom: "0.25rem",
      }}
    >
      {tabs.length === 0 ? (
        <div
          data-testid="tab-bar-empty"
          style={{ color: "#6b7280", padding: "0.35rem 0.5rem" }}
        >
          No open documents
        </div>
      ) : (
        tabs.map((tab) => {
          const isActive = tab.id === activeDocumentId;
          return (
            <div
              data-active={isActive || undefined}
              data-testid={`tab-${tab.id}`}
              key={tab.id}
              style={{
                alignItems: "center",
                background: isActive ? "#ffffff" : "#f8fafc",
                border: `1px solid ${isActive ? "#94a3b8" : "#e2e8f0"}`,
                borderBottom: isActive
                  ? "1px solid #ffffff"
                  : "1px solid #e2e8f0",
                borderRadius: "0.35rem 0.35rem 0 0",
                display: "inline-flex",
                gap: "0.35rem",
                marginBottom: "-1px",
                maxWidth: "18rem",
                padding: "0.25rem 0.4rem",
              }}
            >
              <button
                onClick={() => onSelectTab?.(tab.id)}
                style={{
                  background: "none",
                  border: "none",
                  color: isActive ? "#0f172a" : "#334155",
                  cursor: "pointer",
                  fontSize: "0.85rem",
                  overflow: "hidden",
                  textAlign: "left",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={tab.path}
                type="button"
              >
                {tab.title}
              </button>
              <button
                aria-label={`Close ${tab.title}`}
                data-testid={`tab-close-${tab.id}`}
                onClick={() => onCloseTab?.(tab.id)}
                style={{
                  background: "none",
                  border: "none",
                  color: "#64748b",
                  cursor: "pointer",
                  fontSize: "0.85rem",
                  lineHeight: 1,
                }}
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
