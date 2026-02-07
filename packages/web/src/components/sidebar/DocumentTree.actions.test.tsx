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

    const renameButton = container.querySelector(
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

    const newFolderAction = container.querySelector(
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

    const copyLinkAction = container.querySelector(
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
      root.unmount();
    });
  });
});
