import { EditorState, type StateField } from "@codemirror/state";
import type { DecorationSet } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import {
  activeLines,
  codeBlockDecorations,
  headingPreviewDecorations,
  inlineEmphasisDecorations,
  inlineLinkDecorations,
  isLineActive,
  livePreview,
  mathPreviewDecorations,
  tablePreviewDecorations,
  taskBlockquoteHrDecorations,
} from "./extension.js";

describe("livePreview", () => {
  it("loads extension and initializes active-line tracking", () => {
    const state = EditorState.create({
      doc: "# Title\n\nBody",
      extensions: [livePreview()],
    });

    expect(state.field(activeLines)).toBe(1);
    expect(isLineActive(state, 1)).toBe(true);
    expect(isLineActive(state, 2)).toBe(false);
  });

  it("composes all module decorations on unfocused lines", () => {
    const source = [
      "active",
      "# Heading",
      "**bold** and [Scriptum](https://scriptum.dev)",
      "| Name | Role |",
      "| --- | --- |",
      "| Ada | Agent |",
      "- [x] done",
      "> quoted",
      "---",
      "```ts",
      "const x = 1",
      "```",
      "$$",
      "x^2",
      "$$",
      "Inline $x^2$",
    ].join("\n");

    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, headingPreviewDecorations, 2)).toBe(true);
    expect(hasDecorationOnLine(state, inlineEmphasisDecorations, 3)).toBe(true);
    expect(hasDecorationOnLine(state, inlineLinkDecorations, 3)).toBe(true);
    expect(hasDecorationOnLine(state, tablePreviewDecorations, 4)).toBe(true);
    expect(hasDecorationOnLine(state, taskBlockquoteHrDecorations, 7)).toBe(
      true,
    );
    expect(hasDecorationOnLine(state, codeBlockDecorations, 10)).toBe(true);
    expect(hasDecorationOnLine(state, mathPreviewDecorations, 13)).toBe(true);
  });

  it("skips all preview decorations for an active multi-line selection", () => {
    const source = [
      "# Heading",
      "**bold** and [Scriptum](https://scriptum.dev)",
      "| Name | Role |",
      "| --- | --- |",
      "| Ada | Agent |",
      "- [x] done",
      "> quoted",
      "---",
      "```ts",
      "const x = 1",
      "```",
      "$$",
      "x^2",
      "$$",
      "Inline $x^2$",
    ].join("\n");

    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0, head: source.length },
      extensions: [livePreview()],
    });

    expect(countDecorations(state, headingPreviewDecorations)).toBe(0);
    expect(countDecorations(state, inlineEmphasisDecorations)).toBe(0);
    expect(countDecorations(state, inlineLinkDecorations)).toBe(0);
    expect(countDecorations(state, tablePreviewDecorations)).toBe(0);
    expect(countDecorations(state, taskBlockquoteHrDecorations)).toBe(0);
    expect(countDecorations(state, codeBlockDecorations)).toBe(0);
    expect(countDecorations(state, mathPreviewDecorations)).toBe(0);
  });
});

function hasDecorationOnLine(
  state: EditorState,
  field: StateField<DecorationSet>,
  lineNumber: number,
): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(field);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}

function countDecorations(
  state: EditorState,
  field: StateField<DecorationSet>,
): number {
  let count = 0;
  const decorations = state.field(field);

  decorations.between(0, state.doc.length, () => {
    count += 1;
  });

  return count;
}
