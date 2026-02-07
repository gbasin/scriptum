export interface BreadcrumbSegment {
  label: string;
  path: string;
}

export interface BreadcrumbProps {
  path: string;
  workspaceLabel: string;
  onNavigate?: (path: string | null) => void;
}

export function buildBreadcrumbSegments(path: string): BreadcrumbSegment[] {
  const segments = path
    .split("/")
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
  const breadcrumbs: BreadcrumbSegment[] = [];

  for (let index = 0; index < segments.length; index += 1) {
    breadcrumbs.push({
      label: segments[index],
      path: segments.slice(0, index + 1).join("/"),
    });
  }

  return breadcrumbs;
}

const MAX_BREADCRUMB_LABEL_LENGTH = 24;

export function truncateBreadcrumbLabel(
  label: string,
  maxLength = MAX_BREADCRUMB_LABEL_LENGTH,
): string {
  if (label.length <= maxLength) {
    return label;
  }
  return `${label.slice(0, Math.max(1, maxLength - 1))}…`;
}

export function Breadcrumb({ path, workspaceLabel, onNavigate }: BreadcrumbProps) {
  const segments = buildBreadcrumbSegments(path);
  const rootLabel = truncateBreadcrumbLabel(workspaceLabel);

  const triggerNavigate = (nextPath: string | null) => {
    onNavigate?.(nextPath);
  };

  return (
    <nav
      aria-label="Document breadcrumb"
      data-testid="breadcrumb"
      style={{
        color: "#475569",
        fontSize: "0.82rem",
        marginBottom: "0.75rem",
        marginTop: "0.45rem",
      }}
    >
      <button
        data-testid="breadcrumb-root"
        onClick={() => triggerNavigate(null)}
        style={{
          background: "none",
          border: "none",
          color: "inherit",
          cursor: onNavigate ? "pointer" : "default",
          margin: 0,
          maxWidth: "14rem",
          overflow: "hidden",
          padding: 0,
          textAlign: "inherit",
          textOverflow: "ellipsis",
          verticalAlign: "bottom",
          whiteSpace: "nowrap",
        }}
        title={workspaceLabel}
        type="button"
      >
        {rootLabel}
      </button>
      {segments.map((segment) => {
        const label = truncateBreadcrumbLabel(segment.label);
        return (
          <span data-testid={`breadcrumb-${segment.path}`} key={segment.path}>
            {" / "}
            <button
              data-testid={`breadcrumb-segment-${segment.path}`}
              onClick={() => triggerNavigate(segment.path)}
              style={{
                background: "none",
                border: "none",
                color: "inherit",
                cursor: onNavigate ? "pointer" : "default",
                margin: 0,
                maxWidth: "14rem",
                overflow: "hidden",
                padding: 0,
                textAlign: "inherit",
                textOverflow: "ellipsis",
                verticalAlign: "bottom",
                whiteSpace: "nowrap",
              }}
              title={segment.label}
              type="button"
            >
              {label}
            </button>
          </span>
        );
      })}
    </nav>
  );
}
