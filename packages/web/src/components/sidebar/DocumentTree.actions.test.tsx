// @vitest-environment jsdom

import type { Document } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DocumentTree } from "./DocumentTree";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeDocument(
  overrides: Partial<Document> & {
    id: string;
    path: string;
  },
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

describe("DocumentTree workspace actions", () => {
  it("starts inline rename from context menu and commits with Enter", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onRenameDocument =
      vi.fn<(documentId: string, nextPath: string) => void>();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <DocumentTree
          activeDocumentId={null}
          documents={[makeDocument({ id: "doc-1", path: "docs/readme.md" })]}
          onRenameDocument={onRenameDocument}
        />,
      );
    });

    const nodeButton = container.querySelector(
      '[data-testid="tree-node-docs/readme.md"] button',
    ) as HTMLButtonElement | null;
    expect(nodeButton).not.toBeNull();

    act(() => {
      nodeButton?.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 24,
          clientY: 36,
        }),
      );
    });

    const renameButton = document.querySelector(
      '[data-testid="context-action-rename"]',
    ) as HTMLButtonElement | null;
    expect(renameButton).not.toBeNull();

    act(() => {
      renameButton?.click();
    });

    const renameInput = container.querySelector(
      '[data-testid="tree-rename-input-doc-1"]',
    ) as HTMLInputElement | null;
    expect(renameInput).not.toBeNull();
    expect(renameInput?.getAttribute("aria-label")).toBe("Rename document");

    act(() => {
      const valueSetter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value",
      )?.set;
      valueSetter?.call(renameInput, "docs/overview.md");
      renameInput?.dispatchEvent(new Event("input", { bubbles: true }));
      renameInput?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Enter" }),
      );
    });

    expect(onRenameDocument).toHaveBeenCalledWith("doc-1", "docs/overview.md");

    act(() => {
      root.unmount();
    });
  });

  it("emits extended document context actions", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onContextMenuAction = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <DocumentTree
          activeDocumentId={null}
          documents={[makeDocument({ id: "doc-1", path: "docs/readme.md" })]}
          onContextMenuAction={onContextMenuAction}
        />,
      );
    });

    const nodeButton = container.querySelector(
      '[data-testid="tree-node-docs/readme.md"] button',
    ) as HTMLButtonElement | null;
    expect(nodeButton).not.toBeNull();

    act(() => {
      nodeButton?.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 24,
          clientY: 36,
        }),
      );
    });

    const newFolderAction = document.querySelector(
      '[data-testid="context-action-new-folder"]',
    ) as HTMLButtonElement | null;
    expect(newFolderAction).not.toBeNull();

    act(() => {
      newFolderAction?.click();
    });

    expect(onContextMenuAction).toHaveBeenLastCalledWith(
      "new-folder",
      expect.objectContaining({ id: "doc-1" }),
    );

    act(() => {
      nodeButton?.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 28,
          clientY: 40,
        }),
      );
    });

    const copyLinkAction = document.querySelector(
      '[data-testid="context-action-copy-link"]',
    ) as HTMLButtonElement | null;
    expect(copyLinkAction).not.toBeNull();

    act(() => {
      copyLinkAction?.click();
    });

    expect(onContextMenuAction).toHaveBeenLastCalledWith(
      "copy-link",
      expect.objectContaining({ id: "doc-1" }),
    );

    act(() => {
      nodeButton?.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 32,
          clientY: 44,
        }),
      );
    });

    const archiveAction = document.querySelector(
      '[data-testid="context-action-archive"]',
    ) as HTMLButtonElement | null;
    expect(archiveAction).not.toBeNull();
    expect(
      document.querySelector('[data-testid="context-action-unarchive"]'),
    ).toBeNull();

    act(() => {
      archiveAction?.click();
    });

    expect(onContextMenuAction).toHaveBeenLastCalledWith(
      "archive",
      expect.objectContaining({ id: "doc-1" }),
    );

    act(() => {
      root.unmount();
    });
  });

  it("shows unarchive action for archived documents", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onContextMenuAction = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <DocumentTree
          activeDocumentId={null}
          documents={[
            makeDocument({
              archivedAt: "2026-01-02T00:00:00.000Z",
              id: "doc-1",
              path: "docs/readme.md",
            }),
          ]}
          onContextMenuAction={onContextMenuAction}
        />,
      );
    });

    const nodeButton = container.querySelector(
      '[data-testid="tree-node-docs/readme.md"] button',
    ) as HTMLButtonElement | null;
    expect(nodeButton).not.toBeNull();

    act(() => {
      nodeButton?.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 24,
          clientY: 36,
        }),
      );
    });

    expect(
      document.querySelector('[data-testid="context-action-archive"]'),
    ).toBeNull();
    const unarchiveAction = document.querySelector(
      '[data-testid="context-action-unarchive"]',
    ) as HTMLButtonElement | null;
    expect(unarchiveAction).not.toBeNull();

    act(() => {
      unarchiveAction?.click();
    });

    expect(onContextMenuAction).toHaveBeenLastCalledWith(
      "unarchive",
      expect.objectContaining({ id: "doc-1" }),
    );

    act(() => {
      root.unmount();
    });
  });

  it("supports keyboard arrow navigation with roving tabindex", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onDocumentSelect = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <DocumentTree
          activeDocumentId={null}
          documents={[
            makeDocument({ id: "doc-a", path: "docs/api.md" }),
            makeDocument({ id: "doc-r", path: "docs/readme.md" }),
            makeDocument({ id: "doc-c", path: "changelog.md" }),
          ]}
          onDocumentSelect={onDocumentSelect}
        />,
      );
    });

    const docsItem = container.querySelector(
      '[data-testid="tree-node-docs"]',
    ) as HTMLLIElement | null;
    const apiItem = container.querySelector(
      '[data-testid="tree-node-docs/api.md"]',
    ) as HTMLLIElement | null;
    const changelogItem = container.querySelector(
      '[data-testid="tree-node-changelog.md"]',
    ) as HTMLLIElement | null;

    expect(docsItem).not.toBeNull();
    expect(apiItem).not.toBeNull();
    expect(changelogItem).not.toBeNull();
    expect(docsItem?.tabIndex).toBe(0);
    expect(apiItem?.tabIndex).toBe(-1);

    act(() => {
      docsItem?.focus();
      docsItem?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "ArrowDown" }),
      );
    });
    expect(document.activeElement).toBe(apiItem);

    act(() => {
      apiItem?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "ArrowLeft" }),
      );
    });
    expect(document.activeElement).toBe(docsItem);

    act(() => {
      docsItem?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "ArrowLeft" }),
      );
    });
    expect(docsItem?.getAttribute("aria-expanded")).toBe("false");

    act(() => {
      docsItem?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }),
      );
    });
    expect(docsItem?.getAttribute("aria-expanded")).toBe("true");

    act(() => {
      docsItem?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "End" }),
      );
    });
    expect(document.activeElement).toBe(changelogItem);

    act(() => {
      changelogItem?.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Enter" }),
      );
    });
    expect(onDocumentSelect).toHaveBeenCalledWith("doc-c");

    act(() => {
      root.unmount();
    });
  });
});
