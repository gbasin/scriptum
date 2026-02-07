import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { tablePreviewDecorations } from "./table.js";

describe("table live preview", () => {
  it("renders GFM tables with alignment metadata on unfocused lines", () => {
    const source = [
      "active",
      "| Name | Role | Score |",
      "| :--- | :---: | ---: |",
      "| Ada | Agent | 42 |",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const tables = collectTableWidgets(state);
    expect(tables).toEqual([
      {
        alignments: ["left", "center", "right"],
        headers: ["Name", "Role", "Score"],
      },
    ]);
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("keeps active line raw for markdown table syntax", () => {
    const source = [
      "| Name | Role |",
      "| :--- | :--: |",
      "| Ada | Agent |",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("| Name |") },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 1)).toBe(false);
    expect(hasDecorationOnLine(state, 2)).toBe(false);
  });
});

function collectTableWidgets(state: EditorState): Array<{
  headers: string[];
  alignments: string[];
}> {
  const tables: Array<{ headers: string[]; alignments: string[] }> = [];
  const decorations = state.field(tablePreviewDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (
      value.spec as {
        widget?: {
          kind?: unknown;
          headers?: unknown;
          alignments?: unknown;
        };
      }
    ).widget;
    if (!widget || widget.kind !== "table") {
      return;
    }
    if (!Array.isArray(widget.headers) || !Array.isArray(widget.alignments)) {
      return;
    }

    tables.push({
      alignments: widget.alignments.filter(
        (alignment): alignment is string => typeof alignment === "string",
      ),
      headers: widget.headers.filter(
        (header): header is string => typeof header === "string",
      ),
    });
  });

  return tables;
}

function hasDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(tablePreviewDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
