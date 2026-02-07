export interface BreadcrumbSegment {
  label: string;
  path: string;
}

export interface BreadcrumbProps {
  path: string;
  workspaceLabel: string;
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

export function Breadcrumb({ path, workspaceLabel }: BreadcrumbProps) {
  const segments = buildBreadcrumbSegments(path);

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
      <span data-testid="breadcrumb-root">{workspaceLabel}</span>
      {segments.map((segment) => (
        <span data-testid={`breadcrumb-${segment.path}`} key={segment.path}>
          {" / "}
          <span>{segment.label}</span>
        </span>
      ))}
    </nav>
  );
}
