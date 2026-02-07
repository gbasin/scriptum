// @vitest-environment jsdom

import type { Document } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  collectWorkspaceTags,
  filterDocumentsByTag,
  TagsList,
  tagChipTestId,
  toggleTagSelection,
} from "./TagsList";

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

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("collectWorkspaceTags", () => {
  it("collects unique non-empty tags in sorted order", () => {
    const documents = [
      makeDocument({ id: "doc-a", path: "docs/a.md", tags: ["sync", "agents"] }),
      makeDocument({ id: "doc-b", path: "docs/b.md", tags: ["agents", ""] }),
      makeDocument({ id: "doc-c", path: "docs/c.md", tags: ["review"] }),
    ];

    expect(collectWorkspaceTags(documents)).toEqual(["agents", "review", "sync"]);
  });
});

describe("filterDocumentsByTag", () => {
  it("returns all documents when no active tag is selected", () => {
    const documents = [
      makeDocument({ id: "doc-a", path: "docs/a.md", tags: ["sync"] }),
      makeDocument({ id: "doc-b", path: "docs/b.md", tags: ["review"] }),
    ];

    expect(filterDocumentsByTag(documents, null).map((document) => document.id)).toEqual([
      "doc-a",
      "doc-b",
    ]);
  });

  it("returns only documents matching the selected tag", () => {
    const documents = [
      makeDocument({ id: "doc-a", path: "docs/a.md", tags: ["sync", "agents"] }),
      makeDocument({ id: "doc-b", path: "docs/b.md", tags: ["review"] }),
      makeDocument({ id: "doc-c", path: "docs/c.md", tags: ["agents"] }),
    ];

    expect(
      filterDocumentsByTag(documents, "agents").map((document) => document.id),
    ).toEqual(["doc-a", "doc-c"]);
  });
});

describe("toggleTagSelection", () => {
  it("toggles the active tag off when the same tag is clicked", () => {
    expect(toggleTagSelection("sync", "sync")).toBeNull();
  });

  it("sets the active tag when a different tag is clicked", () => {
    expect(toggleTagSelection("sync", "agents")).toBe("agents");
  });
});

describe("TagsList", () => {
  it("renders chips with active state and deterministic test ids", () => {
    const html = renderToString(
      <TagsList activeTag="sync" tags={["agents", "sync"]} />,
    );

    expect(html).toContain("Tags");
    expect(html).toContain("sidebar-tags-list");
    expect(html).toContain(">sync<");
    expect(html).toContain(`sidebar-tag-chip-${tagChipTestId("sync")}`);
    expect(html).toContain("aria-pressed=\"true\"");
  });

  it("emits selected and cleared tag values on chip clicks", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onTagSelect = vi.fn<(tag: string | null) => void>();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <TagsList
          activeTag={null}
          onTagSelect={onTagSelect}
          tags={["agents", "sync"]}
        />,
      );
    });

    const syncChip = container.querySelector(
      `[data-testid="sidebar-tag-chip-${tagChipTestId("sync")}"]`,
    ) as HTMLButtonElement | null;
    expect(syncChip).not.toBeNull();

    act(() => {
      syncChip?.click();
    });
    expect(onTagSelect).toHaveBeenLastCalledWith("sync");

    act(() => {
      root.render(
        <TagsList
          activeTag="sync"
          onTagSelect={onTagSelect}
          tags={["agents", "sync"]}
        />,
      );
    });

    const activeSyncChip = container.querySelector(
      `[data-testid="sidebar-tag-chip-${tagChipTestId("sync")}"]`,
    ) as HTMLButtonElement | null;
    expect(activeSyncChip).not.toBeNull();

    act(() => {
      activeSyncChip?.click();
    });
    expect(onTagSelect).toHaveBeenLastCalledWith(null);

    act(() => {
      root.unmount();
    });
  });
});
