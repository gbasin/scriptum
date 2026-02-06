import type { Document } from "@scriptum/shared";
import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  buildTree,
  DocumentTree,
  fileIcon,
} from "./DocumentTree";

function makeDoc(overrides: Partial<Document> & { id: string; path: string }): Document {
  return {
    workspaceId: "ws-1",
    title: overrides.path.split("/").pop() ?? "",
    tags: [],
    headSeq: 0,
    etag: `etag-${overrides.id}`,
    archivedAt: null,
    deletedAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

const DOCUMENTS: Document[] = [
  makeDoc({ id: "d1", path: "docs/readme.md" }),
  makeDoc({ id: "d2", path: "docs/api.md" }),
  makeDoc({ id: "d3", path: "notes/todo.md" }),
  makeDoc({ id: "d4", path: "changelog.md" }),
  makeDoc({ id: "d5", path: "docs/guides/getting-started.md" }),
];

describe("buildTree", () => {
  it("builds a nested tree from flat document paths", () => {
    const tree = buildTree(DOCUMENTS);
    const topNames = tree.map((n) => n.name);

    // Folders first (sorted), then files
    expect(topNames).toEqual(["docs", "notes", "changelog.md"]);
  });

  it("sorts folders before files at each level", () => {
    const tree = buildTree(DOCUMENTS);
    const docsNode = tree.find((n) => n.name === "docs")!;
    const childNames = docsNode.children.map((n) => n.name);

    // "guides" folder first, then files alphabetically
    expect(childNames).toEqual(["guides", "api.md", "readme.md"]);
  });

  it("nests deeply", () => {
    const tree = buildTree(DOCUMENTS);
    const guides = tree
      .find((n) => n.name === "docs")!
      .children.find((n) => n.name === "guides")!;

    expect(guides.children).toHaveLength(1);
    expect(guides.children[0].name).toBe("getting-started.md");
    expect(guides.children[0].document?.id).toBe("d5");
  });

  it("returns empty array for no documents", () => {
    expect(buildTree([])).toEqual([]);
  });

  it("handles root-level files", () => {
    const tree = buildTree([makeDoc({ id: "d1", path: "readme.md" })]);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe("readme.md");
    expect(tree[0].document?.id).toBe("d1");
  });
});

describe("fileIcon", () => {
  it("returns markdown icon for .md files", () => {
    expect(fileIcon("readme.md")).toBe("\u{1F4DD}");
    expect(fileIcon("notes.markdown")).toBe("\u{1F4DD}");
  });

  it("returns clipboard icon for .json files", () => {
    expect(fileIcon("config.json")).toBe("\u{1F4CB}");
  });

  it("returns gear icon for .yaml/.yml/.toml files", () => {
    expect(fileIcon("config.yaml")).toBe("\u{2699}");
    expect(fileIcon("config.yml")).toBe("\u{2699}");
    expect(fileIcon("config.toml")).toBe("\u{2699}");
  });

  it("returns generic icon for unknown extensions", () => {
    expect(fileIcon("file.txt")).toBe("\u{1F4C4}");
  });
});

describe("DocumentTree", () => {
  it("renders document tree with folder and file nodes", () => {
    const html = renderToString(
      <DocumentTree
        activeDocumentId="d1"
        documents={DOCUMENTS}
        onDocumentSelect={() => undefined}
      />
    );

    expect(html).toContain("Document tree");
    expect(html).toContain("data-testid=\"document-tree\"");
    expect(html).toContain("readme.md");
    expect(html).toContain("api.md");
    expect(html).toContain("changelog.md");
    expect(html).toContain("todo.md");
    // getting-started.md is in a nested folder (docs/guides) that is not
    // auto-expanded (only top-level folders auto-expand), so it won't
    // appear in server-rendered output. The node IS in the tree data.
    expect(html).toContain("guides");
  });

  it("marks active document", () => {
    const html = renderToString(
      <DocumentTree
        activeDocumentId="d1"
        documents={DOCUMENTS}
        onDocumentSelect={() => undefined}
      />
    );

    // The active node should have data-active attribute and highlighted background
    expect(html).toContain('data-active="true"');
    expect(html).toContain("#e0f2fe");
  });

  it("shows empty state when no documents", () => {
    const html = renderToString(
      <DocumentTree
        activeDocumentId={null}
        documents={[]}
        onDocumentSelect={() => undefined}
      />
    );

    expect(html).toContain("document-tree-empty");
    expect(html).toContain("No documents yet");
  });

  it("renders folder icons for directories", () => {
    const html = renderToString(
      <DocumentTree
        activeDocumentId={null}
        documents={DOCUMENTS}
        onDocumentSelect={() => undefined}
      />
    );

    // Open folder icon (top-level folders auto-expand)
    expect(html).toContain("\u{1F4C2}");
    // Markdown file icon
    expect(html).toContain("\u{1F4DD}");
  });

  it("uses tree ARIA roles", () => {
    const html = renderToString(
      <DocumentTree
        activeDocumentId={null}
        documents={DOCUMENTS}
        onDocumentSelect={() => undefined}
      />
    );

    expect(html).toContain('role="tree"');
    expect(html).toContain('role="treeitem"');
  });
});
