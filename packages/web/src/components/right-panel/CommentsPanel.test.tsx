// @vitest-environment jsdom

import type { Document } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { InlineCommentThread } from "../../lib/inline-comments";
import { commentsStoreKey } from "../../store/comments";
import { CommentsPanel } from "./CommentsPanel";

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

function makeThread(
  id: string,
  overrides: Partial<InlineCommentThread> = {},
): InlineCommentThread {
  return {
    endOffsetUtf16: 12,
    id,
    messages: [
      {
        authorName: "Ada",
        bodyMd: "Please revisit this paragraph.",
        createdAt: "2026-01-02T00:00:00.000Z",
        id: `${id}-msg-1`,
        isOwn: false,
      },
    ],
    startOffsetUtf16: 2,
    status: "open",
    ...overrides,
  };
}

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("CommentsPanel", () => {
  it("renders empty state when there are no threads", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <CommentsPanel
          activeDocumentId={null}
          documents={[makeDocument({ id: "doc-1", path: "docs/readme.md" })]}
          threadsByDocumentKey={{}}
          workspaceId="ws-1"
        />,
      );
    });

    expect(
      container.querySelector('[data-testid="comments-panel-empty"]')
        ?.textContent,
    ).toContain("No comments yet.");

    act(() => {
      root.unmount();
    });
  });

  it("prefers current document threads when available", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    const documents = [
      makeDocument({ id: "doc-a", path: "docs/auth.md", title: "Auth" }),
      makeDocument({ id: "doc-b", path: "docs/search.md", title: "Search" }),
    ];

    act(() => {
      root.render(
        <CommentsPanel
          activeDocumentId="doc-a"
          documents={documents}
          threadsByDocumentKey={{
            [commentsStoreKey("ws-1", "doc-a")]: [makeThread("thread-a")],
            [commentsStoreKey("ws-1", "doc-b")]: [makeThread("thread-b")],
          }}
          workspaceId="ws-1"
        />,
      );
    });

    expect(
      container.querySelector('[data-testid="comments-panel-thread-thread-a"]'),
    ).not.toBeNull();
    expect(
      container.querySelector('[data-testid="comments-panel-thread-thread-b"]'),
    ).toBeNull();

    act(() => {
      root.unmount();
    });
  });

  it("falls back to workspace threads when active document has none", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    const documents = [
      makeDocument({ id: "doc-a", path: "docs/auth.md", title: "Auth" }),
      makeDocument({ id: "doc-b", path: "docs/search.md", title: "Search" }),
    ];

    act(() => {
      root.render(
        <CommentsPanel
          activeDocumentId="doc-a"
          documents={documents}
          threadsByDocumentKey={{
            [commentsStoreKey("ws-1", "doc-b")]: [makeThread("thread-b")],
          }}
          workspaceId="ws-1"
        />,
      );
    });

    expect(
      container.querySelector('[data-testid="comments-panel-thread-thread-b"]'),
    ).not.toBeNull();

    act(() => {
      root.unmount();
    });
  });

  it("emits thread selection with document and thread ids", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onThreadSelect =
      vi.fn<(documentId: string, threadId: string) => void>();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <CommentsPanel
          activeDocumentId={null}
          documents={[makeDocument({ id: "doc-a", path: "docs/auth.md" })]}
          onThreadSelect={onThreadSelect}
          threadsByDocumentKey={{
            [commentsStoreKey("ws-1", "doc-a")]: [makeThread("thread-a")],
          }}
          workspaceId="ws-1"
        />,
      );
    });

    const threadButton = container.querySelector(
      '[data-testid="comments-panel-thread-thread-a"]',
    ) as HTMLButtonElement | null;
    expect(threadButton).not.toBeNull();

    act(() => {
      threadButton?.click();
    });

    expect(onThreadSelect).toHaveBeenCalledWith("doc-a", "thread-a");

    act(() => {
      root.unmount();
    });
  });
});
