import type { Document } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import {
  buildOpenDocumentTabs,
  buildUntitledPath,
  nextDocumentIdAfterClose,
  titleFromPath,
} from "./document";

function makeDocument(
  overrides: Partial<Document> & {
    id: string;
    path: string;
    title: string;
    workspaceId: string;
  },
): Document {
  return {
    id: overrides.id,
    workspaceId: overrides.workspaceId,
    path: overrides.path,
    title: overrides.title,
    tags: [],
    headSeq: 0,
    etag: `etag-${overrides.id}`,
    archivedAt: null,
    deletedAt: null,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

describe("document tab helpers", () => {
  it("builds workspace-scoped tabs from open documents", () => {
    const tabs = buildOpenDocumentTabs(
      [
        makeDocument({
          id: "doc-a",
          workspaceId: "ws-1",
          path: "docs/a.md",
          title: "a.md",
        }),
        makeDocument({
          id: "doc-b",
          workspaceId: "ws-2",
          path: "docs/b.md",
          title: "b.md",
        }),
      ],
      "ws-1",
      "doc-a",
      "docs/a.md",
    );

    expect(tabs).toEqual([
      {
        id: "doc-a",
        path: "docs/a.md",
        title: "a.md",
      },
    ]);
  });

  it("injects a fallback active tab when the active route doc is missing", () => {
    const tabs = buildOpenDocumentTabs(
      [],
      "ws-1",
      "doc-active",
      "docs/new-note.md",
    );

    expect(tabs).toEqual([
      {
        id: "doc-active",
        path: "docs/new-note.md",
        title: "new-note",
      },
    ]);
  });

  it("chooses the left neighbor after closing an active tab", () => {
    const next = nextDocumentIdAfterClose(
      [
        { id: "doc-1", path: "docs/1.md", title: "1.md" },
        { id: "doc-2", path: "docs/2.md", title: "2.md" },
        { id: "doc-3", path: "docs/3.md", title: "3.md" },
      ],
      "doc-2",
    );

    expect(next).toBe("doc-1");
  });

  it("returns null when the last tab closes", () => {
    const next = nextDocumentIdAfterClose(
      [{ id: "doc-1", path: "docs/1.md", title: "1.md" }],
      "doc-1",
    );
    expect(next).toBeNull();
  });

  it("builds untitled paths by incrementing numeric suffixes", () => {
    const nextPath = buildUntitledPath(
      new Set(["untitled-1.md", "untitled-2.md", "notes/untitled-1.md"]),
    );
    expect(nextPath).toBe("untitled-3.md");
  });

  it("derives display titles from markdown file paths", () => {
    expect(titleFromPath("docs/intro/getting-started.md")).toBe(
      "getting-started",
    );
    expect(titleFromPath("README.MD")).toBe("README");
    expect(titleFromPath("plain-name")).toBe("plain-name");
  });
});
