// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import { footnotePreview, footnotePreviewDecorations } from "./footnotes.js";

describe("footnotePreview", () => {
  it("renders inline references and a footnote section widget", () => {
    const source = ["Title", "See footnote [^a].", "", "[^a]: Alpha note"].join("\n");
    const state = EditorState.create({
      doc: source,
      extensions: [footnotePreview()],
    });

    expect(collectWidgetKinds(state)).toContain("footnote-reference");
    expect(collectWidgetKinds(state)).toContain("footnote-section");
    expect(replacedDefinitionLineCount(state)).toBe(1);
    expect(footnoteSectionText(state)).toContain("Footnotes");
    expect(footnoteSectionText(state)).toContain("Alpha note");
  });

  it("keeps raw footnote definition visible when actively editing definition lines", () => {
    const source = ["Title", "See footnote [^a].", "", "[^a]: Alpha note"].join("\n");
    const definitionLine = source.indexOf("[^a]:");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: definitionLine },
      extensions: [footnotePreview()],
    });

    expect(collectWidgetKinds(state)).toContain("footnote-reference");
    expect(collectWidgetKinds(state)).not.toContain("footnote-section");
    expect(replacedDefinitionLineCount(state)).toBe(0);
  });

  it("supports multi-line footnote definitions", () => {
    const source = [
      "Line with ref [^a].",
      "",
      "[^a]: first line",
      "    second line",
      "    third line",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      extensions: [footnotePreview()],
    });

    expect(footnoteSectionText(state)).toContain("first line");
    expect(footnoteSectionText(state)).toContain("second line");
    expect(footnoteSectionText(state)).toContain("third line");
  });
});

function collectWidgetKinds(state: EditorState): string[] {
  const decorations = state.field(footnotePreviewDecorations);
  const kinds = new Set<string>();

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: string } }).widget;
    if (typeof widget?.kind === "string") {
      kinds.add(widget.kind);
    }
  });

  return Array.from(kinds.values()).sort();
}

function replacedDefinitionLineCount(state: EditorState): number {
  const decorations = state.field(footnotePreviewDecorations);
  let count = 0;

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const spec = value.spec as {
      block?: boolean;
      widget?: unknown;
    };
    if (spec.block && !spec.widget) {
      count += 1;
    }
  });

  return count;
}

function footnoteSectionText(state: EditorState): string {
  const decorations = state.field(footnotePreviewDecorations);
  let text = "";

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: string; toDOM?: () => HTMLElement } })
      .widget;
    if (widget?.kind !== "footnote-section" || typeof widget.toDOM !== "function") {
      return;
    }

    text = widget.toDOM().textContent ?? "";
  });

  return text;
}
