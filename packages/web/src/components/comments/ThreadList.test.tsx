// @vitest-environment jsdom

import { act } from "react";
import type { ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThreadList } from "./ThreadList";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function flushMicrotasks(): Promise<void> {
  return Promise.resolve();
}

function renderThreadList(props: ComponentProps<typeof ThreadList>) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(<ThreadList {...props} />);
  });

  return {
    container,
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

describe("ThreadList", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.restoreAllMocks();
  });

  it("renders threaded messages with markdown formatting", () => {
    const harness = renderThreadList({
      workspaceId: "ws-1",
      thread: {
        id: "thread-1",
        docId: "doc-1",
        sectionId: "section-1",
        startOffsetUtf16: 3,
        endOffsetUtf16: 14,
        status: "open",
        version: 1,
        createdBy: "user-1",
        createdAt: "2026-02-07T01:00:00.000Z",
        resolvedAt: null,
      },
      messages: [
        {
          id: "msg-1",
          threadId: "thread-1",
          author: "Alex",
          bodyMd: "**Bold** text with `code` and [link](https://example.com).",
          createdAt: "2026-02-07T01:01:00.000Z",
          editedAt: null,
        },
      ],
    });

    expect(harness.container.textContent).toContain("Alex");
    expect(harness.container.textContent).toContain("Bold");
    expect(harness.container.querySelector("strong")?.textContent).toContain("Thread");
    expect(harness.container.querySelector("code")?.textContent).toBe("code");
    const link = harness.container.querySelector("a");
    expect(link?.getAttribute("href")).toBe("https://example.com");

    harness.unmount();
  });

  it("submits a reply and emits the created message", async () => {
    const replyToThread = vi
      .fn<
        (
          workspaceId: string,
          threadId: string,
          bodyMd: string,
        ) => Promise<ComponentProps<typeof ThreadList>["messages"][number]>
      >()
      .mockResolvedValue({
        id: "msg-2",
        threadId: "thread-1",
        author: "You",
        bodyMd: "Follow-up",
        createdAt: "2026-02-07T01:02:00.000Z",
        editedAt: null,
      });
    const onMessageCreated = vi.fn<(message: ComponentProps<typeof ThreadList>["messages"][number]) => void>();

    const harness = renderThreadList({
      workspaceId: "ws-1",
      thread: {
        id: "thread-1",
        docId: "doc-1",
        sectionId: "section-1",
        startOffsetUtf16: 3,
        endOffsetUtf16: 14,
        status: "open",
        version: 1,
        createdBy: "user-1",
        createdAt: "2026-02-07T01:00:00.000Z",
        resolvedAt: null,
      },
      messages: [],
      onMessageCreated,
      replyToThread,
    });

    const input = harness.container.querySelector(
      "[data-testid=\"thread-list-reply-input\"]",
    ) as HTMLTextAreaElement | null;
    expect(input).not.toBeNull();

    act(() => {
      if (input) {
        const setValue = Object.getOwnPropertyDescriptor(
          window.HTMLTextAreaElement.prototype,
          "value",
        )?.set;
        setValue?.call(input, "Follow-up");
        input.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });

    const submit = harness.container.querySelector(
      "[data-testid=\"thread-list-reply-submit\"]",
    ) as HTMLButtonElement | null;
    expect(submit).not.toBeNull();

    await act(async () => {
      submit?.click();
      await flushMicrotasks();
    });

    expect(replyToThread).toHaveBeenCalledWith("ws-1", "thread-1", "Follow-up");
    expect(onMessageCreated).toHaveBeenCalledWith({
      id: "msg-2",
      threadId: "thread-1",
      author: "You",
      bodyMd: "Follow-up",
      createdAt: "2026-02-07T01:02:00.000Z",
      editedAt: null,
    });

    harness.unmount();
  });

  it("transitions resolved/open status using resolve and reopen handlers", async () => {
    const resolveThread = vi
      .fn<
        (
          workspaceId: string,
          threadId: string,
          ifVersion: number,
        ) => Promise<ComponentProps<typeof ThreadList>["thread"]>
      >()
      .mockResolvedValue({
        id: "thread-1",
        docId: "doc-1",
        sectionId: "section-1",
        startOffsetUtf16: 3,
        endOffsetUtf16: 14,
        status: "resolved",
        version: 2,
        createdBy: "user-1",
        createdAt: "2026-02-07T01:00:00.000Z",
        resolvedAt: "2026-02-07T01:03:00.000Z",
      });
    const reopenThread = vi
      .fn<
        (
          workspaceId: string,
          threadId: string,
          ifVersion: number,
        ) => Promise<ComponentProps<typeof ThreadList>["thread"]>
      >()
      .mockResolvedValue({
        id: "thread-1",
        docId: "doc-1",
        sectionId: "section-1",
        startOffsetUtf16: 3,
        endOffsetUtf16: 14,
        status: "open",
        version: 3,
        createdBy: "user-1",
        createdAt: "2026-02-07T01:00:00.000Z",
        resolvedAt: null,
      });
    const onThreadUpdated = vi.fn<(thread: ComponentProps<typeof ThreadList>["thread"]) => void>();

    const harness = renderThreadList({
      workspaceId: "ws-1",
      thread: {
        id: "thread-1",
        docId: "doc-1",
        sectionId: "section-1",
        startOffsetUtf16: 3,
        endOffsetUtf16: 14,
        status: "open",
        version: 1,
        createdBy: "user-1",
        createdAt: "2026-02-07T01:00:00.000Z",
        resolvedAt: null,
      },
      messages: [],
      onThreadUpdated,
      reopenThread,
      resolveThread,
    });

    const resolveButton = harness.container.querySelector(
      "[data-testid=\"thread-list-resolve\"]",
    ) as HTMLButtonElement | null;
    expect(resolveButton).not.toBeNull();

    await act(async () => {
      resolveButton?.click();
      await flushMicrotasks();
    });

    expect(resolveThread).toHaveBeenCalledWith("ws-1", "thread-1", 1);
    expect(harness.container.querySelector("[data-testid=\"thread-list-resolved-note\"]")).not.toBeNull();

    const reopenButton = harness.container.querySelector(
      "[data-testid=\"thread-list-reopen\"]",
    ) as HTMLButtonElement | null;
    expect(reopenButton).not.toBeNull();

    await act(async () => {
      reopenButton?.click();
      await flushMicrotasks();
    });

    expect(reopenThread).toHaveBeenCalledWith("ws-1", "thread-1", 2);
    expect(onThreadUpdated).toHaveBeenCalledTimes(2);

    harness.unmount();
  });
});
