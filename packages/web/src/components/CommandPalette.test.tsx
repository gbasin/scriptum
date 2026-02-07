// @vitest-environment jsdom

import type { Document, Workspace } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  buildCommandPaletteItems,
  CommandPalette,
  filterCommandPaletteItems,
  nextPaletteIndex,
} from "./CommandPalette";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeWorkspace(id: string, name: string): Workspace {
  return {
    createdAt: "2026-01-01T00:00:00.000Z",
    etag: `${id}-etag`,
    id,
    name,
    role: "owner",
    slug: name.toLowerCase(),
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

function makeDocument(
  overrides: Partial<Document> & {
    id: string;
    path: string;
    workspaceId: string;
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
    ...overrides,
  };
}

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("buildCommandPaletteItems", () => {
  it("builds recent, file, and command sections scoped to active workspace", () => {
    const documents = [
      makeDocument({ id: "doc-a", path: "docs/a.md", workspaceId: "ws-1" }),
      makeDocument({ id: "doc-b", path: "notes/b.md", workspaceId: "ws-2" }),
      makeDocument({ id: "doc-c", path: "docs/c.md", workspaceId: "ws-1" }),
    ];
    const items = buildCommandPaletteItems({
      activeWorkspaceId: "ws-1",
      documents,
      openDocumentIds: ["doc-b", "doc-a", "doc-c"],
      workspaces: [
        makeWorkspace("ws-1", "Alpha"),
        makeWorkspace("ws-2", "Beta"),
      ],
    });

    expect(items.slice(0, 4).map((item) => item.id)).toEqual([
      "recent:doc-c",
      "recent:doc-a",
      "file:doc-a",
      "file:doc-c",
    ]);
    expect(items.some((item) => item.id === "file:doc-b")).toBe(false);
    expect(items.some((item) => item.id === "command:create-workspace")).toBe(
      true,
    );
    expect(items.some((item) => item.id === "command:settings")).toBe(true);
  });
});

describe("filterCommandPaletteItems", () => {
  it("applies case-insensitive multi-token filtering", () => {
    const items = buildCommandPaletteItems({
      activeWorkspaceId: "ws-1",
      documents: [
        makeDocument({ id: "doc-a", path: "docs/a.md", workspaceId: "ws-1" }),
      ],
      openDocumentIds: [],
      workspaces: [makeWorkspace("ws-1", "Alpha")],
    });

    expect(filterCommandPaletteItems(items, "open settings")).toEqual([
      expect.objectContaining({ id: "command:settings" }),
    ]);
    expect(filterCommandPaletteItems(items, "FILE docs")).toEqual([
      expect.objectContaining({ id: "file:doc-a" }),
    ]);
  });
});

describe("nextPaletteIndex", () => {
  it("wraps around when moving up and down", () => {
    expect(nextPaletteIndex(-1, "down", 3)).toBe(0);
    expect(nextPaletteIndex(2, "down", 3)).toBe(0);
    expect(nextPaletteIndex(0, "up", 3)).toBe(2);
    expect(nextPaletteIndex(-1, "up", 3)).toBe(2);
    expect(nextPaletteIndex(0, "down", 0)).toBe(-1);
  });
});

describe("CommandPalette", () => {
  it("renders trigger with Cmd+K shortcut hint", () => {
    const html = renderToString(
      <MemoryRouter>
        <CommandPalette
          activeWorkspaceId="ws-1"
          documents={[]}
          onCreateWorkspace={() => undefined}
          openDocumentIds={[]}
          workspaces={[makeWorkspace("ws-1", "Alpha")]}
        />
      </MemoryRouter>,
    );

    expect(html).toContain("Search files, commands, recent docs");
    expect(html).toContain("Cmd+K");
  });

  it("opens with Cmd+K and selects create workspace via keyboard navigation", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onCreateWorkspace = vi.fn<() => void>();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter>
          <CommandPalette
            activeWorkspaceId={null}
            documents={[]}
            onCreateWorkspace={onCreateWorkspace}
            openDocumentIds={[]}
            workspaces={[makeWorkspace("ws-1", "Alpha")]}
          />
        </MemoryRouter>,
      );
    });

    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          bubbles: true,
          key: "k",
          metaKey: true,
        }),
      );
    });
    expect(
      container.querySelector('[data-testid="command-palette"]'),
    ).not.toBeNull();

    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "ArrowDown" }),
      );
    });

    const createItemSelector =
      '[data-testid="command-palette-item-command:create-workspace"]';

    let createItem = container.querySelector(createItemSelector);
    if (createItem?.getAttribute("aria-selected") !== "true") {
      act(() => {
        window.dispatchEvent(
          new KeyboardEvent("keydown", { bubbles: true, key: "ArrowDown" }),
        );
      });
      createItem = container.querySelector(createItemSelector);
    }

    if (createItem?.getAttribute("aria-selected") !== "true") {
      act(() => {
        window.dispatchEvent(
          new KeyboardEvent("keydown", { bubbles: true, key: "ArrowDown" }),
        );
      });
      createItem = container.querySelector(createItemSelector);
    }

    expect(createItem?.getAttribute("aria-selected")).toBe("true");

    act(() => {
      if (createItem) {
        (createItem as HTMLElement).focus();
      }
    });

    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Enter" }),
      );
    });

    expect(onCreateWorkspace).toHaveBeenCalledTimes(1);
    expect(
      container.querySelector('[data-testid="command-palette"]'),
    ).toBeNull();

    act(() => {
      root.unmount();
    });
  });
});
