// @vitest-environment jsdom

import { act } from "react";
import type { ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { buildInlineDiffSegments, DiffView, type AuthorshipRange } from "./DiffView";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function renderDiffView(props: ComponentProps<typeof DiffView>) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(<DiffView {...props} />);
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

describe("buildInlineDiffSegments", () => {
  it("builds removed/added segments between current and historical snapshots", () => {
    expect(buildInlineDiffSegments("Hello world", "Hello Scriptum")).toEqual([
      { kind: "unchanged", text: "Hello " },
      { kind: "removed", text: "world" },
      { kind: "added", text: "Scriptum" },
    ]);
  });
});

describe("DiffView", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  });

  it("renders authorship legend and highlighted content in read-only CodeMirror", () => {
    const rangeAlice: AuthorshipRange = { from: 0, to: 5 };
    const rangeBob: AuthorshipRange = { from: 6, to: 11 };
    const authorshipMap = new Map<AuthorshipRange, string>([
      [rangeAlice, "Alice"],
      [rangeBob, "Bob"],
    ]);

    const harness = renderDiffView({
      authorshipMap,
      currentContent: "Hello world",
      historicalContent: "Hello world",
      viewMode: "authorship",
    });

    expect(harness.container.textContent).toContain("Author-colored highlights");
    expect(
      harness.container.querySelector(
        "[data-testid=\"history-diffview-author-Alice\"]",
      )?.textContent,
    ).toContain("Alice");
    expect(
      harness.container.querySelector(
        "[data-testid=\"history-diffview-author-Bob\"]",
      )?.textContent,
    ).toContain("Bob");

    const editor = harness.container.querySelector(
      "[data-testid=\"history-diffview-editor\"]",
    );
    expect(editor?.textContent).toContain("Hello world");
    expect(harness.container.querySelector("[data-author=\"Alice\"]")).not.toBeNull();
    expect(harness.container.querySelector("[data-author=\"Bob\"]")).not.toBeNull();

    harness.unmount();
  });

  it("renders diff mode with added and removed markers", () => {
    const harness = renderDiffView({
      authorshipMap: new Map(),
      currentContent: "Hello world",
      historicalContent: "Hello Scriptum",
      viewMode: "diff",
    });

    expect(harness.container.textContent).toContain("Diff from current");
    expect(harness.container.querySelector("[data-kind=\"removed\"]")?.textContent).toBe(
      "world",
    );
    expect(harness.container.querySelector("[data-kind=\"added\"]")?.textContent).toBe(
      "Scriptum",
    );

    harness.unmount();
  });

  it("shows empty-state message when historical snapshot matches current content", () => {
    const harness = renderDiffView({
      authorshipMap: new Map(),
      currentContent: "No changes",
      historicalContent: "No changes",
      viewMode: "diff",
    });

    expect(
      harness.container.querySelector("[data-testid=\"history-diffview-diff-empty\"]")
        ?.textContent,
    ).toContain("Selected snapshot matches current version.");

    harness.unmount();
  });
});
