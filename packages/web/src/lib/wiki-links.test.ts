import type { Document } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import { buildIncomingBacklinks } from "./wiki-links";

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
    updatedAt: "2026-01-03T00:00:00.000Z",
    workspaceId: "ws-alpha",
    ...overrides,
  };
}

describe("buildIncomingBacklinks", () => {
  it("resolves path, filename, and title aliases while skipping broken targets", () => {
    const documents = [
      makeDocument({
        id: "doc-auth",
        path: "docs/auth.md",
        title: "Auth",
      }),
      makeDocument({
        id: "doc-path",
        path: "notes/by-path.md",
        title: "By Path",
        bodyMd: "See [[docs/auth.md]] for details.",
      }),
      makeDocument({
        id: "doc-file",
        path: "notes/by-file.md",
        title: "By File",
        bodyMd: "Related setup lives in [[auth]].",
      }),
      makeDocument({
        id: "doc-title",
        path: "notes/by-title.md",
        title: "By Title",
        bodyMd: "Context in [[Auth|Authentication design]].",
      }),
      makeDocument({
        id: "doc-broken",
        path: "notes/broken.md",
        title: "Broken",
        bodyMd: "Broken link [[missing-target]].",
      }),
    ];

    expect(buildIncomingBacklinks(documents, "doc-auth")).toEqual([
      {
        snippet: "[[auth]]",
        sourceDocumentId: "doc-file",
        sourcePath: "notes/by-file.md",
        sourceTitle: "By File",
      },
      {
        snippet: "[[docs/auth.md]]",
        sourceDocumentId: "doc-path",
        sourcePath: "notes/by-path.md",
        sourceTitle: "By Path",
      },
      {
        snippet: "[[Auth|Authentication design]]",
        sourceDocumentId: "doc-title",
        sourcePath: "notes/by-title.md",
        sourceTitle: "By Title",
      },
    ]);
  });

  it("ignores escaped, nested, and empty wiki-link patterns", () => {
    const documents = [
      makeDocument({
        id: "doc-auth",
        path: "docs/auth.md",
        title: "Auth",
      }),
      makeDocument({
        id: "doc-escaped",
        path: "notes/escaped.md",
        title: "Escaped",
        bodyMd: "Escape sequence \\[[Auth]] should stay plain text.",
      }),
      makeDocument({
        id: "doc-nested",
        path: "notes/nested.md",
        title: "Nested",
        bodyMd: "Nested links like [[outer [[Auth]]]] are invalid.",
      }),
      makeDocument({
        id: "doc-empty",
        path: "notes/empty.md",
        title: "Empty",
        bodyMd: "Ignore empty link [[   ]].",
      }),
      makeDocument({
        id: "doc-valid",
        path: "notes/valid.md",
        title: "Valid",
        bodyMd: "Keep [[Auth]] as a backlink.",
      }),
    ];

    expect(buildIncomingBacklinks(documents, "doc-auth")).toEqual([
      {
        snippet: "[[Auth]]",
        sourceDocumentId: "doc-valid",
        sourcePath: "notes/valid.md",
        sourceTitle: "Valid",
      },
    ]);
  });
});
