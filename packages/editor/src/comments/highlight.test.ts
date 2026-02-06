import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  commentHighlightExtension,
  commentHighlightState,
  setCommentHighlightRanges,
} from "./highlight";

interface HighlightEntry {
  className: string;
  from: number;
  to: number;
}

describe("comment highlights", () => {
  it("renders open and resolved highlight ranges", () => {
    const state = EditorState.create({
      doc: "alpha bravo charlie",
      extensions: [commentHighlightExtension()],
    }).update({
      effects: [
        setCommentHighlightRanges.of([
          {
            from: 0,
            status: "open",
            threadId: "t-open",
            to: 5,
          },
          {
            from: 6,
            status: "resolved",
            threadId: "t-resolved",
            to: 11,
          },
        ]),
      ],
    }).state;

    const highlights = collectHighlights(state);
    expect(highlights).toEqual([
      {
        className: "cm-commentHighlight cm-commentHighlight-open",
        from: 0,
        to: 5,
      },
      {
        className: "cm-commentHighlight cm-commentHighlight-resolved",
        from: 6,
        to: 11,
      },
    ]);
  });

  it("replaces previous ranges when a new range set is applied", () => {
    let state = EditorState.create({
      doc: "line one",
      extensions: [commentHighlightExtension()],
    });

    state = state.update({
      effects: [
        setCommentHighlightRanges.of([
          {
            from: 0,
            status: "open",
            threadId: "t-1",
            to: 4,
          },
        ]),
      ],
    }).state;
    expect(collectHighlights(state).length).toBe(1);

    state = state.update({
      effects: [
        setCommentHighlightRanges.of([
          {
            from: 5,
            status: "resolved",
            threadId: "t-2",
            to: 8,
          },
        ]),
      ],
    }).state;

    expect(collectHighlights(state)).toEqual([
      {
        className: "cm-commentHighlight cm-commentHighlight-resolved",
        from: 5,
        to: 8,
      },
    ]);
  });

  it("maps highlight ranges across document changes", () => {
    let state = EditorState.create({
      doc: "first line\nsecond line",
      extensions: [commentHighlightExtension()],
    });

    state = state.update({
      effects: [
        setCommentHighlightRanges.of([
          {
            from: "first line\n".length,
            status: "open",
            threadId: "t-second",
            to: "first line\nsecond".length,
          },
        ]),
      ],
    }).state;

    state = state.update({
      changes: {
        from: 0,
        to: 0,
        insert: "new line\n",
      },
    }).state;

    const highlights = collectHighlights(state);
    expect(highlights).toEqual([
      {
        className: "cm-commentHighlight cm-commentHighlight-open",
        from: "new line\nfirst line\n".length,
        to: "new line\nfirst line\nsecond".length,
      },
    ]);
  });

  it("drops invalid ranges", () => {
    const state = EditorState.create({
      doc: "abc",
      extensions: [commentHighlightExtension()],
    }).update({
      effects: [
        setCommentHighlightRanges.of([
          {
            from: 2,
            status: "open",
            threadId: "invalid",
            to: 2,
          },
        ]),
      ],
    }).state;

    expect(collectHighlights(state)).toEqual([]);
  });
});

function collectHighlights(state: EditorState): HighlightEntry[] {
  const highlights: HighlightEntry[] = [];
  const decorations = state.field(commentHighlightState).decorations;

  decorations.between(0, state.doc.length, (from, to, value) => {
    const className = (value.spec as { class?: string }).class;
    if (!className) {
      return;
    }

    highlights.push({
      className,
      from,
      to,
    });
  });

  return highlights;
}
