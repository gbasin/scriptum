import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  type CommentDecorationStatus,
  commentGutterExtension,
  commentGutterState,
  setCommentGutterRanges,
} from "./gutter";

interface GutterEntry {
  line: number;
  status: CommentDecorationStatus;
}

describe("comment gutter", () => {
  it("renders markers for lines covered by comment ranges", () => {
    const doc = ["line one", "line two", "line three"].join("\n");
    const lineTwoStart = "line one\n".length;
    const lineThreeStart = "line one\nline two\n".length;

    const state = EditorState.create({
      doc,
      extensions: [commentGutterExtension()],
    }).update({
      effects: [
        setCommentGutterRanges.of([
          {
            from: lineTwoStart + 1,
            status: "open",
            threadId: "thread-open",
            to: lineThreeStart + 4,
          },
        ]),
      ],
    }).state;

    expect(collectGutterEntries(state)).toEqual([
      { line: 2, status: "open" },
      { line: 3, status: "open" },
    ]);
  });

  it("prefers open status when mixed markers are on the same line", () => {
    const state = EditorState.create({
      doc: "line one\nline two",
      extensions: [commentGutterExtension()],
    }).update({
      effects: [
        setCommentGutterRanges.of([
          {
            from: 0,
            status: "resolved",
            threadId: "thread-resolved",
            to: 4,
          },
          {
            from: 2,
            status: "open",
            threadId: "thread-open",
            to: 6,
          },
        ]),
      ],
    }).state;

    expect(collectGutterEntries(state)).toEqual([{ line: 1, status: "open" }]);
  });

  it("maps marker lines when document content shifts", () => {
    let state = EditorState.create({
      doc: "alpha\nbravo",
      extensions: [commentGutterExtension()],
    });

    state = state.update({
      effects: [
        setCommentGutterRanges.of([
          {
            from: "alpha\n".length,
            status: "resolved",
            threadId: "thread-bravo",
            to: "alpha\nbravo".length,
          },
        ]),
      ],
    }).state;
    expect(collectGutterEntries(state)).toEqual([
      { line: 2, status: "resolved" },
    ]);

    state = state.update({
      changes: {
        from: 0,
        to: 0,
        insert: "new\n",
      },
    }).state;

    expect(collectGutterEntries(state)).toEqual([
      { line: 3, status: "resolved" },
    ]);
  });
});

function collectGutterEntries(state: EditorState): GutterEntry[] {
  const entries: GutterEntry[] = [];
  const markers = state.field(commentGutterState).markers;

  markers.between(0, state.doc.length, (from, _to, marker) => {
    const status = (marker as { status?: unknown }).status;
    if (status !== "open" && status !== "resolved") {
      return;
    }
    entries.push({
      line: state.doc.lineAt(from).number,
      status,
    });
  });

  entries.sort((left, right) => left.line - right.line);
  return entries;
}
