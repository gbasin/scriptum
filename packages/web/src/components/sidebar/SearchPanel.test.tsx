// @vitest-environment jsdom

import type { Document } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  buildSearchPanelResults,
  filterSearchResults,
  highlightText,
  isSearchPanelShortcut,
  SearchPanel,
  type SearchPanelResult,
} from "./SearchPanel";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeDocument(
  overrides: Partial<Document> & { id: string; path: string },
): Document {
  return {
    archivedAt: null,
    createdAt: "2026-01-01T00:00:00.000Z",
    deletedAt: null,
    etag: `etag-${overrides.id}`,
    headSeq: 0,
    tags: [],
    title: overrides.path.split("/").pop() ?? "",
    updatedAt: "2026-01-01T00:00:00.000Z",
    workspaceId: "ws-1",
    ...overrides,
  };
}

const SEARCH_RESULTS: SearchPanelResult[] = [
  {
    author: "Alice",
    documentId: "doc-a",
    documentPath: "docs/auth.md",
    documentTitle: "Auth",
    id: "doc-a:12",
    lineNumber: 12,
    snippet: "Authentication state machine and reconnect flow",
    tags: ["auth", "sync"],
    updatedAt: "2026-01-10T10:00:00.000Z",
  },
  {
    author: "Bob",
    documentId: "doc-b",
    documentPath: "docs/search.md",
    documentTitle: "Search",
    id: "doc-b:4",
    lineNumber: 4,
    snippet: "Search panel fixture with highlighted snippet context",
    tags: ["search"],
    updatedAt: "2026-01-20T10:00:00.000Z",
  },
];

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("buildSearchPanelResults", () => {
  it("indexes non-empty markdown body lines with line numbers", () => {
    const documents = [
      makeDocument({
        bodyMd: "# Heading\n\nSearchable paragraph\nFinal line",
        id: "doc-a",
        path: "docs/a.md",
        tags: ["alpha"],
        title: "A",
        updatedAt: "2026-01-03T00:00:00.000Z",
      }),
    ];

    expect(buildSearchPanelResults(documents)).toEqual([
      {
        author: "Unknown",
        documentId: "doc-a",
        documentPath: "docs/a.md",
        documentTitle: "A",
        id: "doc-a:1",
        lineNumber: 1,
        snippet: "# Heading",
        tags: ["alpha"],
        updatedAt: "2026-01-03T00:00:00.000Z",
      },
      {
        author: "Unknown",
        documentId: "doc-a",
        documentPath: "docs/a.md",
        documentTitle: "A",
        id: "doc-a:3",
        lineNumber: 3,
        snippet: "Searchable paragraph",
        tags: ["alpha"],
        updatedAt: "2026-01-03T00:00:00.000Z",
      },
      {
        author: "Unknown",
        documentId: "doc-a",
        documentPath: "docs/a.md",
        documentTitle: "A",
        id: "doc-a:4",
        lineNumber: 4,
        snippet: "Final line",
        tags: ["alpha"],
        updatedAt: "2026-01-03T00:00:00.000Z",
      },
    ]);
  });

  it("falls back to title/path snippet when body markdown is empty", () => {
    const documents = [
      makeDocument({
        bodyMd: "",
        id: "doc-empty",
        path: "docs/empty.md",
        title: "Empty",
      }),
    ];

    expect(buildSearchPanelResults(documents)).toEqual([
      {
        author: "Unknown",
        documentId: "doc-empty",
        documentPath: "docs/empty.md",
        documentTitle: "Empty",
        id: "doc-empty:1",
        lineNumber: 1,
        snippet: "Empty (docs/empty.md)",
        tags: [],
        updatedAt: "2026-01-01T00:00:00.000Z",
      },
    ]);
  });

  it("derives authors from document metadata with priority order", () => {
    const documents = [
      {
        ...makeDocument({
          bodyMd: "First line",
          id: "doc-edited",
          path: "docs/edited.md",
          title: "Edited",
        }),
        createdBy: "Creator",
        lastEditedBy: "Editor",
      } as Document,
      {
        ...makeDocument({
          bodyMd: "Second line",
          id: "doc-created",
          path: "docs/created.md",
          title: "Created",
        }),
        createdBy: "Creator",
      } as Document,
      {
        ...makeDocument({
          bodyMd: "Third line",
          id: "doc-nested",
          path: "docs/nested.md",
          title: "Nested",
        }),
        metadata: { author_name: "Nested Author" },
      } as Document,
    ];

    expect(
      buildSearchPanelResults(documents).map((result) => result.author),
    ).toEqual(["Editor", "Creator", "Nested Author"]);
  });
});

describe("filterSearchResults", () => {
  it("filters by query, tag, author, and date range", () => {
    expect(
      filterSearchResults(SEARCH_RESULTS, "search", {
        author: "Bob",
        dateFrom: "2026-01-15",
        dateTo: "2026-01-22",
        tag: "search",
      }).map((result) => result.id),
    ).toEqual(["doc-b:4"]);
  });
});

describe("highlightText", () => {
  it("returns match and non-match segments", () => {
    expect(highlightText("search panel search", "search")).toEqual([
      { isMatch: true, text: "search" },
      { isMatch: false, text: " panel " },
      { isMatch: true, text: "search" },
    ]);
  });
});

describe("isSearchPanelShortcut", () => {
  it("matches Cmd/Ctrl+Shift+F only", () => {
    expect(
      isSearchPanelShortcut({
        ctrlKey: false,
        key: "f",
        metaKey: true,
        shiftKey: true,
      }),
    ).toBe(true);
    expect(
      isSearchPanelShortcut({
        ctrlKey: true,
        key: "F",
        metaKey: false,
        shiftKey: true,
      }),
    ).toBe(true);
    expect(
      isSearchPanelShortcut({
        ctrlKey: false,
        key: "f",
        metaKey: true,
        shiftKey: false,
      }),
    ).toBe(false);
  });
});

describe("SearchPanel", () => {
  it("renders highlighted snippets and filter controls", () => {
    const html = renderToString(<SearchPanel results={SEARCH_RESULTS} />);

    expect(html).toContain("Search");
    expect(html).toContain("search-panel-query");
    expect(html).toContain("search-panel-filter-tag");
    expect(html).toContain("search-panel-filter-author");
    expect(html).toContain(
      "Search panel fixture with highlighted snippet context",
    );
  });

  it("invokes onResultSelect for clicked result", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onResultSelect = vi.fn<(result: SearchPanelResult) => void>();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <SearchPanel
          onResultSelect={onResultSelect}
          results={SEARCH_RESULTS}
        />,
      );
    });

    const queryInput = container.querySelector(
      '[data-testid="search-panel-query"]',
    ) as HTMLInputElement | null;
    expect(queryInput).not.toBeNull();

    act(() => {
      if (queryInput) {
        queryInput.value = "auth";
        queryInput.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });

    const resultButton = container.querySelector(
      '[data-testid="search-panel-result-doc-a:12"]',
    ) as HTMLButtonElement | null;
    expect(resultButton).not.toBeNull();

    act(() => {
      resultButton?.click();
    });

    expect(onResultSelect).toHaveBeenCalledWith(
      expect.objectContaining({ id: "doc-a:12" }),
    );

    act(() => {
      root.unmount();
    });
  });

  it("renders loading skeletons when loading is true", () => {
    const html = renderToString(
      <SearchPanel loading results={SEARCH_RESULTS} />,
    );

    expect(html).toContain('data-testid="search-panel-loading"');
    expect(html).not.toContain('data-testid="search-panel-empty"');
  });
});
