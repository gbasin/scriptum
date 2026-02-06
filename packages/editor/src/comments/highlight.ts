import {
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";

export type CommentDecorationStatus = "open" | "resolved";

export interface CommentDecorationRange {
  readonly threadId: string;
  readonly from: number;
  readonly to: number;
  readonly status: CommentDecorationStatus;
}

interface CommentHighlightState {
  readonly ranges: readonly CommentDecorationRange[];
  readonly decorations: DecorationSet;
}

const OPEN_STATUS: CommentDecorationStatus = "open";

export const setCommentHighlightRanges =
  StateEffect.define<readonly CommentDecorationRange[]>();

function sanitizeStatus(status: CommentDecorationStatus | undefined): CommentDecorationStatus {
  if (status === "resolved") {
    return status;
  }
  return OPEN_STATUS;
}

function normalizeRanges(
  state: EditorState,
  ranges: readonly CommentDecorationRange[]
): readonly CommentDecorationRange[] {
  const maxPosition = state.doc.length;
  const normalized: CommentDecorationRange[] = [];

  for (const range of ranges) {
    const from = Math.max(0, Math.min(range.from, maxPosition));
    const to = Math.max(0, Math.min(range.to, maxPosition));
    if (to <= from) {
      continue;
    }

    normalized.push({
      from,
      status: sanitizeStatus(range.status),
      threadId: range.threadId,
      to,
    });
  }

  return normalized;
}

function mapRangesThroughChanges(
  state: EditorState,
  ranges: readonly CommentDecorationRange[],
  changes: { mapPos: (pos: number, assoc?: number) => number }
): readonly CommentDecorationRange[] {
  const mappedRanges = ranges.map((range) => {
    const from = changes.mapPos(range.from, 1);
    const to = changes.mapPos(range.to, -1);
    return {
      ...range,
      from: Math.min(from, to),
      to: Math.max(from, to),
    };
  });

  return normalizeRanges(state, mappedRanges);
}

function buildHighlightDecorations(
  ranges: readonly CommentDecorationRange[]
): DecorationSet {
  if (ranges.length === 0) {
    return Decoration.none;
  }

  return Decoration.set(
    ranges.map((range) => ({
      from: range.from,
      to: range.to,
      value: Decoration.mark({
        class:
          range.status === "resolved"
            ? "cm-commentHighlight cm-commentHighlight-resolved"
            : "cm-commentHighlight cm-commentHighlight-open",
      }),
    })),
    true
  );
}

export const commentHighlightState = StateField.define<CommentHighlightState>({
  create: () => ({
    decorations: Decoration.none,
    ranges: [],
  }),
  provide: (field) =>
    EditorView.decorations.from(field, (value) => value.decorations),
  update(current, transaction) {
    let nextRanges = current.ranges;

    for (const effect of transaction.effects) {
      if (effect.is(setCommentHighlightRanges)) {
        nextRanges = normalizeRanges(transaction.state, effect.value);
      }
    }

    if (nextRanges === current.ranges && transaction.docChanged) {
      nextRanges = mapRangesThroughChanges(
        transaction.state,
        current.ranges,
        transaction.changes
      );
    }

    if (nextRanges === current.ranges) {
      return current;
    }

    return {
      decorations: buildHighlightDecorations(nextRanges),
      ranges: nextRanges,
    };
  },
});

const commentHighlightTheme = EditorView.baseTheme({
  ".cm-commentHighlight": {
    borderRadius: "2px",
  },
  ".cm-commentHighlight-open": {
    backgroundColor: "rgba(250, 204, 21, 0.32)",
  },
  ".cm-commentHighlight-resolved": {
    backgroundColor: "rgba(156, 163, 175, 0.24)",
  },
});

export function commentHighlightExtension(): Extension {
  return [commentHighlightState, commentHighlightTheme];
}

