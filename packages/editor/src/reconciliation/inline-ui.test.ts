// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";

import {
  RECONCILIATION_KEEP_BOTH_SEPARATOR,
  reconciliationInlineExtension,
  setReconciliationInlineEntries,
  type ReconciliationChoice,
  type ReconciliationInlineResolution,
} from "./inline-ui";

describe("reconciliationInlineExtension", () => {
  it("renders both versions with author attribution and action buttons", () => {
    const view = createView("before current after");
    view.dispatch({
      effects: [
        setReconciliationInlineEntries.of([
          {
            id: "rec-1",
            sectionId: "section/auth",
            from: 7,
            to: 14,
            versionA: {
              authorId: "gary",
              authorName: "Gary",
              content: "OAuth flow version A",
            },
            versionB: {
              authorId: "claude-1",
              authorName: "claude-1",
              content: "OAuth flow version B",
            },
          },
        ]),
      ],
    });

    const root = getReconciliationRoot(view, "rec-1");
    expect(root).not.toBeNull();
    expect(root?.textContent).toContain("Version A by Gary");
    expect(root?.textContent).toContain("OAuth flow version A");
    expect(root?.textContent).toContain("Version B by claude-1");
    expect(root?.textContent).toContain("OAuth flow version B");
    expect(root?.textContent).toContain("Keep A");
    expect(root?.textContent).toContain("Keep B");
    expect(root?.textContent).toContain("Keep Both");

    view.destroy();
  });

  it("applies Keep A and removes inline reconciliation UI", () => {
    const before = "prefix ";
    const current = "current";
    const after = " suffix";
    const view = createView(`${before}${current}${after}`);

    view.dispatch({
      effects: [
        setReconciliationInlineEntries.of([
          {
            id: "rec-keep-a",
            sectionId: "sec-1",
            from: before.length,
            to: before.length + current.length,
            versionA: {
              authorId: "alice",
              authorName: "Alice",
              content: "resolved-a",
            },
            versionB: {
              authorId: "bob",
              authorName: "Bob",
              content: "resolved-b",
            },
          },
        ]),
      ],
    });

    clickChoice(view, "rec-keep-a", "keep-a");

    expect(view.state.doc.toString()).toBe(`${before}resolved-a${after}`);
    expect(getReconciliationRoot(view, "rec-keep-a")).toBeNull();

    view.destroy();
  });

  it("applies Keep Both with separator and reports resolution metadata", () => {
    const before = "alpha ";
    const current = "middle";
    const after = " omega";
    const resolutions: ReconciliationInlineResolution[] = [];
    const view = createView(`${before}${current}${after}`, {
      onResolve: (resolution) => {
        resolutions.push(resolution);
      },
    });

    view.dispatch({
      effects: [
        setReconciliationInlineEntries.of([
          {
            id: "rec-keep-both",
            sectionId: "sec-auth",
            from: before.length,
            to: before.length + current.length,
            versionA: {
              authorId: "gary",
              authorName: "Gary",
              content: "Version A body",
            },
            versionB: {
              authorId: "claude-1",
              authorName: "claude-1",
              content: "Version B body",
            },
            triggeredAtMs: 12345,
          },
        ]),
      ],
    });

    clickChoice(view, "rec-keep-both", "keep-both");

    const expectedReplacement =
      "Version A body" +
      RECONCILIATION_KEEP_BOTH_SEPARATOR +
      "Version B body";
    expect(view.state.doc.toString()).toBe(`${before}${expectedReplacement}${after}`);
    expect(getReconciliationRoot(view, "rec-keep-both")).toBeNull();
    expect(resolutions).toEqual([
      {
        id: "rec-keep-both",
        sectionId: "sec-auth",
        choice: "keep-both",
        replacement: expectedReplacement,
        from: before.length,
        to: before.length + current.length,
        triggeredAtMs: 12345,
      },
    ]);

    view.destroy();
  });
});

function createView(
  doc: string,
  options?: {
    onResolve?: (resolution: ReconciliationInlineResolution) => void;
  },
): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  return new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [reconciliationInlineExtension(options)],
    }),
  });
}

function getReconciliationRoot(view: EditorView, id: string): HTMLElement | null {
  return view.dom.querySelector(`[data-reconciliation-id="${id}"]`);
}

function clickChoice(
  view: EditorView,
  id: string,
  choice: ReconciliationChoice,
): void {
  const root = getReconciliationRoot(view, id);
  if (!root) {
    throw new Error(`missing reconciliation root for ${id}`);
  }

  const button = root.querySelector<HTMLButtonElement>(
    `button[data-choice="${choice}"]`,
  );
  if (!button) {
    throw new Error(`missing reconciliation button for ${choice}`);
  }

  button.click();
}
