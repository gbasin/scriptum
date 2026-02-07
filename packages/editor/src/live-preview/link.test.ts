import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { inlineLinkDecorations } from "./link.js";

describe("link live preview", () => {
  it("renders markdown links and autolinks on unfocused lines", () => {
    const source = [
      "active line",
      "[Scriptum](https://scriptum.dev) and <https://example.com>",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const widgets = collectInlineWidgetKinds(state);
    expect(widgets).toEqual(["link", "link"]);
    expect(hasDecorationOnLine(state, 1)).toBe(false);
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("renders image previews on unfocused lines", () => {
    const source = [
      "active",
      "![Alt text](https://example.com/image.png)",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const widgets = collectInlineWidgetKinds(state);
    expect(widgets).toEqual(["image"]);
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("keeps active line raw for link and image markdown", () => {
    const source =
      "[raw](https://example.com) ![img](https://example.com/a.png)\nline";
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 1)).toBe(false);
  });
});

function collectInlineWidgetKinds(state: EditorState): string[] {
  const kinds: string[] = [];
  const decorations = state.field(inlineLinkDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: unknown } }).widget;
    if (!widget || (widget.kind !== "link" && widget.kind !== "image")) {
      return;
    }
    kinds.push(widget.kind);
  });

  return kinds;
}

function hasDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(inlineLinkDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
