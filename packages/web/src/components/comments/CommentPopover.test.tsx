// @vitest-environment jsdom

import type { ComponentProps } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CommentPopover, highlightRangesFromThreads } from "./CommentPopover";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function flushMicrotasks(): Promise<void> {
  return Promise.resolve();
}

function renderPopover(props: ComponentProps<typeof CommentPopover>) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  const render = (nextProps: ComponentProps<typeof CommentPopover>) => {
    act(() => {
      root.render(<CommentPopover {...nextProps} />);
    });
  };

  render(props);

  return {
    container,
    rerender: (nextProps: ComponentProps<typeof CommentPopover>) =>
      render(nextProps),
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

describe("highlightRangesFromThreads", () => {
  it("maps thread anchors into highlight ranges", () => {
    expect(
      highlightRangesFromThreads([
        {
          thread: {
            id: "thread-1",
            docId: "doc-1",
            sectionId: null,
            startOffsetUtf16: 10,
            endOffsetUtf16: 18,
            status: "open",
            version: 1,
            createdBy: "user-1",
            createdAt: "2026-02-07T00:00:00.000Z",
            resolvedAt: null,
          },
          messages: [],
        },
      ]),
    ).toEqual([
      {
        from: 10,
        threadId: "thread-1",
        to: 18,
        status: "open",
      },
    ]);
  });
});

describe("CommentPopover", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.restoreAllMocks();
  });

  it("opens from margin button and shows selected text preview", () => {
    const harness = renderPopover({
      workspaceId: "ws-1",
      documentId: "doc-1",
      selection: {
        sectionId: "section-1",
        startOffsetUtf16: 5,
        endOffsetUtf16: 17,
        headSeq: 7,
        selectedText: "Inline selection",
      },
      anchorTopPx: 40,
      activeThread: null,
    });

    const button = harness.container.querySelector(
      '[data-testid="comment-margin-button"]',
    ) as HTMLButtonElement | null;
    expect(button).not.toBeNull();

    act(() => {
      button?.click();
    });

    const popover = harness.container.querySelector(
      '[data-testid="comment-popover"]',
    );
    expect(popover).not.toBeNull();
    expect(
      harness.container.querySelector(
        '[data-testid="comment-selection-preview"]',
      )?.textContent,
    ).toContain("Inline selection");

    harness.unmount();
  });

  it("creates a comment thread through api-client callback", async () => {
    const createThread = vi
      .fn<NonNullable<ComponentProps<typeof CommentPopover>["createThread"]>>()
      .mockResolvedValue({
        thread: {
          id: "thread-1",
          docId: "doc-1",
          sectionId: "section-1",
          startOffsetUtf16: 5,
          endOffsetUtf16: 17,
          status: "open",
          version: 1,
          createdBy: "user-1",
          createdAt: "2026-02-07T01:00:00.000Z",
          resolvedAt: null,
        },
        message: {
          id: "msg-1",
          threadId: "thread-1",
          author: "You",
          bodyMd: "First comment",
          createdAt: "2026-02-07T01:00:00.000Z",
          editedAt: null,
        },
      });
    const onThreadChange =
      vi.fn<
        NonNullable<ComponentProps<typeof CommentPopover>["onThreadChange"]>
      >();

    const harness = renderPopover({
      workspaceId: "ws-1",
      documentId: "doc-1",
      selection: {
        sectionId: "section-1",
        startOffsetUtf16: 5,
        endOffsetUtf16: 17,
        headSeq: 7,
        selectedText: "Inline selection",
      },
      anchorTopPx: 40,
      activeThread: null,
      createThread,
      onThreadChange,
    });

    const button = harness.container.querySelector(
      '[data-testid="comment-margin-button"]',
    ) as HTMLButtonElement | null;
    act(() => {
      button?.click();
    });

    const input = harness.container.querySelector(
      '[data-testid="comment-input"]',
    ) as HTMLTextAreaElement | null;
    expect(input).not.toBeNull();

    act(() => {
      if (input) {
        const setValue = Object.getOwnPropertyDescriptor(
          window.HTMLTextAreaElement.prototype,
          "value",
        )?.set;
        setValue?.call(input, "First comment");
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });

    const submit = harness.container.querySelector(
      '[data-testid="comment-submit"]',
    ) as HTMLButtonElement | null;
    expect(submit).not.toBeNull();

    await act(async () => {
      submit?.click();
      await flushMicrotasks();
    });

    expect(createThread).toHaveBeenCalledWith("ws-1", "doc-1", {
      anchor: {
        sectionId: "section-1",
        startOffsetUtf16: 5,
        endOffsetUtf16: 17,
        headSeq: 7,
      },
      message: "First comment",
    });
    expect(onThreadChange).toHaveBeenCalledTimes(1);
    expect(
      harness.container.querySelector('[data-testid="thread-list"]'),
    ).not.toBeNull();

    harness.unmount();
  });

  it("shows resolved dot marker for resolved active thread", () => {
    const harness = renderPopover({
      workspaceId: "ws-1",
      documentId: "doc-1",
      selection: {
        sectionId: null,
        startOffsetUtf16: 5,
        endOffsetUtf16: 17,
        headSeq: 7,
        selectedText: "Inline selection",
      },
      anchorTopPx: 40,
      activeThread: {
        thread: {
          id: "thread-1",
          docId: "doc-1",
          sectionId: null,
          startOffsetUtf16: 5,
          endOffsetUtf16: 17,
          status: "resolved",
          version: 2,
          createdBy: "user-1",
          createdAt: "2026-02-07T01:00:00.000Z",
          resolvedAt: "2026-02-07T01:05:00.000Z",
        },
        messages: [],
      },
    });

    expect(
      harness.container.querySelector(
        '[data-testid="comment-margin-resolved-dot"]',
      ),
    ).not.toBeNull();

    harness.unmount();
  });
});
