import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { codeBlockDecorations } from "./code-block.js";
import { livePreview } from "./extension.js";

describe("code block live preview", () => {
  it("renders fenced code blocks with language detection on unfocused lines", () => {
    const source = ["active", "", "```ts", "const x = 1", "```"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(collectCodeBlockLanguages(state)).toEqual(["ts"]);
    expect(hasDecorationOnLine(state, 3)).toBe(true);
  });

  it("keeps active fenced code blocks as raw markdown", () => {
    const source = ["active", "", "```ts", "const x = 1", "```"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("```ts") },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 3)).toBe(false);
  });

  it("renders mermaid fenced diagrams on unfocused lines", () => {
    const source = [
      "active",
      "",
      "```mermaid",
      "graph TD",
      "A-->B",
      "```",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(collectCodeBlockWidgetKinds(state)).toContain("mermaid-diagram");
    expect(hasDecorationOnLine(state, 3)).toBe(true);
  });

  it("keeps active mermaid fenced blocks as raw markdown", () => {
    const source = [
      "active",
      "",
      "```mermaid",
      "graph TD",
      "A-->B",
      "```",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("```mermaid") },
      extensions: [livePreview()],
    });

    expect(collectCodeBlockWidgetKinds(state)).not.toContain("mermaid-diagram");
    expect(hasDecorationOnLine(state, 3)).toBe(false);
  });
});

function collectCodeBlockLanguages(state: EditorState): string[] {
  const languages: string[] = [];
  const decorations = state.field(codeBlockDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (
      value.spec as { widget?: { kind?: unknown; language?: unknown } }
    ).widget;
    if (!widget || widget.kind !== "code-block") {
      return;
    }

    if (typeof widget.language === "string") {
      languages.push(widget.language);
    } else {
      languages.push("");
    }
  });

  return languages;
}

function collectCodeBlockWidgetKinds(state: EditorState): string[] {
  const kinds: string[] = [];
  const decorations = state.field(codeBlockDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: unknown } }).widget;
    if (!widget || typeof widget.kind !== "string") {
      return;
    }
    kinds.push(widget.kind);
  });

  return kinds;
}

function hasDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(codeBlockDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
