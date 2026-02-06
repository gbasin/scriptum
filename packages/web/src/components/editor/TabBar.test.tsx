import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Breadcrumb, buildBreadcrumbSegments } from "./Breadcrumb";
import { TabBar, type OpenDocumentTab } from "./TabBar";

const OPEN_TABS: OpenDocumentTab[] = [
  {
    id: "doc-a",
    path: "docs/getting-started.md",
    title: "getting-started.md",
  },
  {
    id: "doc-b",
    path: "notes/ideas.md",
    title: "ideas.md",
  },
];

describe("TabBar", () => {
  it("renders open tabs and marks the active tab", () => {
    const html = renderToString(
      <TabBar
        activeDocumentId="doc-a"
        onCloseTab={() => undefined}
        onSelectTab={() => undefined}
        tabs={OPEN_TABS}
      />
    );

    expect(html).toContain("tab-bar");
    expect(html).toContain("getting-started.md");
    expect(html).toContain("ideas.md");
    expect(html).toContain('data-active="true"');
    expect(html).toContain("tab-close-doc-a");
  });

  it("renders empty state when no tabs are open", () => {
    const html = renderToString(
      <TabBar activeDocumentId={null} tabs={[]} />
    );
    expect(html).toContain("No open documents");
    expect(html).toContain("tab-bar-empty");
  });
});

describe("Breadcrumb", () => {
  it("splits path into breadcrumb segments", () => {
    expect(buildBreadcrumbSegments("docs/guides/start.md")).toEqual([
      { label: "docs", path: "docs" },
      { label: "guides", path: "docs/guides" },
      { label: "start.md", path: "docs/guides/start.md" },
    ]);
  });

  it("renders workspace root and path segments", () => {
    const html = renderToString(
      <Breadcrumb path="docs/guides/start.md" workspaceLabel="Workspace Alpha" />
    );

    expect(html).toContain("Document breadcrumb");
    expect(html).toContain("Workspace Alpha");
    expect(html).toContain("docs");
    expect(html).toContain("guides");
    expect(html).toContain("start.md");
  });
});

