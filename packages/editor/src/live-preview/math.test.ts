import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { mathPreviewDecorations } from "./math.js";

describe("math live preview", () => {
  it("renders inline math on unfocused lines", () => {
    const source = ["active", "Euler identity: $e^{i\\pi}+1=0$"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(collectMathWidgetKinds(state)).toContain("math-inline");
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("renders block math on unfocused lines", () => {
    const source = ["active", "$$", "\\int_0^1 x^2 dx", "$$"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(collectMathWidgetKinds(state)).toContain("math-block");
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("distinguishes inline and block math widgets in the same document", () => {
    const source = [
      "active",
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

    const kinds = collectMathWidgetKinds(state);
    expect(kinds.filter((kind) => kind === "math-inline")).toHaveLength(1);
    expect(kinds.filter((kind) => kind === "math-block")).toHaveLength(1);
  });

  it("supports inline math before block math without crashing decoration ordering", () => {
    const source = [
      "active",
      "Inline $x^2$ first",
      "",
      "$$",
      "\\int_0^1 f(x)\\,dx",
      "$$",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const kinds = collectMathWidgetKinds(state);
    expect(kinds).toContain("math-inline");
    expect(kinds).toContain("math-block");
    expect(kinds).toHaveLength(2);
  });

  it("keeps active math expressions as raw markdown", () => {
    const inlineSource = ["$x^2$", "inactive"].join("\n");
    const inlineState = EditorState.create({
      doc: inlineSource,
      selection: { anchor: inlineSource.indexOf("$x^2$") },
      extensions: [livePreview()],
    });
    expect(hasDecorationOnLine(inlineState, 1)).toBe(false);

    const blockSource = ["active", "$$", "x^2", "$$"].join("\n");
    const blockState = EditorState.create({
      doc: blockSource,
      selection: { anchor: blockSource.indexOf("$$") },
      extensions: [livePreview()],
    });
    expect(hasDecorationOnLine(blockState, 2)).toBe(false);
  });
});

function collectMathWidgetKinds(state: EditorState): string[] {
  const kinds: string[] = [];
  const decorations = state.field(mathPreviewDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: unknown } }).widget;
    if (
      !widget ||
      (widget.kind !== "math-inline" && widget.kind !== "math-block")
    ) {
      return;
    }
    kinds.push(widget.kind);
  });

  return kinds;
}

function hasDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(mathPreviewDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
